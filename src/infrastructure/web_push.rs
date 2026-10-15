//! Restricted Web Push encryption, signing and bounded HTTPS provider transport.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::error::AppError;

const PROVIDER_DEADLINE: Duration = Duration::from_secs(10);

mod config;
#[cfg(feature = "test-failpoints")]
pub use config::FixturePushTransport;
pub use config::WebPushConfig;

/// A provider outcome contains categories only; endpoints, tokens and body never enter logs.
#[derive(Debug, PartialEq)]
pub enum ProviderOutcome {
    Accepted,
    BindingExpired,
    Retryable(&'static str, Option<u32>),
    Definitive(&'static str),
}

#[derive(Clone)]
pub struct WebPushDelivery {
    config: WebPushConfig,
}

impl WebPushDelivery {
    /// Build an optional adapter with matching VAPID keys and restricted destinations.
    pub fn new(config: &WebPushConfig) -> Result<Self, AppError> {
        config.validate()?;
        Ok(Self {
            config: config.clone(),
        })
    }

    /// Bound DNS, TLS and the entire request to ten seconds; redirects and proxies are disabled.
    pub async fn send(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        payload: Vec<u8>,
        ttl: u32,
    ) -> ProviderOutcome {
        tokio::time::timeout(
            PROVIDER_DEADLINE,
            self.send_request(endpoint, p256dh, auth, payload, ttl),
        )
        .await
        .unwrap_or(ProviderOutcome::Retryable("provider_timeout", None))
    }

    async fn send_request(
        &self,
        endpoint: &str,
        p256dh: &str,
        auth: &str,
        payload: Vec<u8>,
        ttl: u32,
    ) -> ProviderOutcome {
        let Ok(url) = reqwest::Url::parse(endpoint) else {
            return ProviderOutcome::Definitive("invalid_endpoint");
        };
        let Some(host) = url.host_str() else {
            return ProviderOutcome::Definitive("invalid_endpoint");
        };
        // Browser push providers use the standard HTTPS origin. The upstream VAPID
        // audience excludes ports; reject nonstandard origins instead of signing a wrong audience.
        if url.scheme() != "https"
            || url.port().is_some_and(|port| port != 443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !self
                .config
                .allowed_hosts
                .iter()
                .any(|allowed| allowed == host)
        {
            return ProviderOutcome::Definitive("invalid_endpoint");
        }

        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(PROVIDER_DEADLINE);
        #[cfg(feature = "test-failpoints")]
        let (client, fixture_addresses) = if let Some(fixture) = self
            .config
            .fixture_transport
            .as_ref()
            .filter(|fixture| fixture.host == host)
        {
            let Ok(certificate) = reqwest::Certificate::from_der(&fixture.certificate) else {
                return ProviderOutcome::Definitive("invalid_fixture");
            };
            (
                client.add_root_certificate(certificate),
                Some(vec![fixture.address]),
            )
        } else {
            (client, None)
        };
        #[cfg(not(feature = "test-failpoints"))]
        let fixture_addresses: Option<Vec<SocketAddr>> = None;
        let addresses = if let Some(addresses) = fixture_addresses {
            addresses
        } else {
            let Ok(addresses) = tokio::net::lookup_host((host, 443)).await else {
                return ProviderOutcome::Retryable("provider_dns", None);
            };
            let addresses: Vec<_> = addresses.collect();
            if !public_destinations(&addresses) {
                return ProviderOutcome::Definitive("private_destination");
            }
            addresses
        };

        // Pin the validated DNS snapshot, retaining the original hostname for TLS
        // verification. A second DNS resolution must not enable rebinding to private services.
        let Ok(client) = client.resolve_to_addrs(host, &addresses).build() else {
            return ProviderOutcome::Definitive("provider_configuration");
        };

        let prepared = (|| {
            let public = web_push_native::p256::PublicKey::from_sec1_bytes(
                &URL_SAFE_NO_PAD.decode(p256dh).ok()?,
            )
            .ok()?;
            let auth = URL_SAFE_NO_PAD.decode(auth).ok()?;
            if auth.len() != 16 {
                return None;
            }
            let auth = web_push_native::Auth::clone_from_slice(&auth);
            let key = web_push_native::jwt_simple::algorithms::ES256KeyPair::from_bytes(
                &URL_SAFE_NO_PAD.decode(&self.config.private_key).ok()?,
            )
            .ok()?;
            let request =
                web_push_native::WebPushBuilder::new(endpoint.parse().ok()?, public, auth)
                    .with_valid_duration(Duration::from_secs(ttl.min(300) as u64))
                    .with_vapid(&key, &self.config.contact)
                    .build(payload)
                    .ok()?;
            Some(request)
        })();
        let Some(request) = prepared else {
            return ProviderOutcome::Definitive("invalid_subscription");
        };

        let (parts, body) = request.into_parts();
        let Ok(response) = client
            .post(url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
        else {
            return ProviderOutcome::Retryable("provider_connection", None);
        };

        match response.status().as_u16() {
            200..=299 => ProviderOutcome::Accepted,
            404 | 410 => ProviderOutcome::BindingExpired,
            429 | 500..=599 => {
                let delay = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| {
                        value.parse::<u32>().ok().or_else(|| {
                            httpdate::parse_http_date(value)
                                .ok()?
                                .duration_since(std::time::SystemTime::now())
                                .ok()
                                .map(|delay| delay.as_secs().min(u32::MAX as u64) as u32)
                        })
                    });
                ProviderOutcome::Retryable("provider_unavailable", delay)
            }
            _ => ProviderOutcome::Definitive("provider_rejected"),
        }
    }
}

fn public_destinations(addresses: &[SocketAddr]) -> bool {
    // Validate the entire DNS snapshot, not only the address a connection happens
    // to choose. The provider client pins this same snapshot for TLS transport.
    !addresses.is_empty()
        && addresses.iter().all(|address| match address.ip() {
            IpAddr::V4(ip) => public_ipv4(ip),
            IpAddr::V6(ip) => public_ipv6(ip),
        })
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [first_octet, second_octet, third_octet, _] = ip.octets();
    let private_network = first_octet == 10
        || first_octet == 172 && (16..=31).contains(&second_octet)
        || first_octet == 192 && second_octet == 168;
    let local_or_shared_network = first_octet == 127
        || first_octet == 100 && (64..=127).contains(&second_octet)
        || first_octet == 169 && second_octet == 254;
    let special_use_network = first_octet == 0
        || first_octet >= 224
        || first_octet == 192 && (second_octet == 0 || second_octet == 88 && third_octet == 99)
        || first_octet == 198
            && (second_octet == 18
                || second_octet == 19
                || second_octet == 51 && third_octet == 100)
        || first_octet == 203 && second_octet == 0 && third_octet == 113;

    !(private_network || local_or_shared_network || special_use_network)
}

fn public_ipv6(ip: Ipv6Addr) -> bool {
    let [first_segment, second_segment, ..] = ip.segments();
    let global_unicast = first_segment & 0xe000 == 0x2000;
    let special_use_network = first_segment == 0x2001
        && (second_segment < 0x200 || second_segment == 0xdb8)
        || first_segment == 0x2002
        || first_segment == 0x3fff && second_segment & 0xf000 == 0;

    global_unicast && !special_use_network
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    use super::WebPushConfig;
    use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;

    #[test]
    fn dns_snapshot_must_contain_only_public_unicast_addresses() {
        let address = |ip: &str| std::net::SocketAddr::new(ip.parse().unwrap(), 443);
        assert!(super::public_destinations(&[
            address("8.8.8.8"),
            address("2606:4700::1111")
        ]));
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "203.0.113.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2002:7f00:1::1",
            "3fff::1",
        ] {
            assert!(
                !super::public_destinations(&[address("8.8.8.8"), address(ip)]),
                "{ip}"
            );
        }
        assert!(!super::public_destinations(&[]));
    }

    #[test]
    fn pure_rust_receiver_decrypts_the_rfc8291_appendix_a_fixture() {
        let ciphertext=URL_SAFE_NO_PAD.decode("DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN").unwrap();
        let secret = web_push_native::p256::SecretKey::from_slice(
            &URL_SAFE_NO_PAD
                .decode("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94")
                .unwrap(),
        )
        .unwrap();
        let auth = web_push_native::Auth::clone_from_slice(
            &URL_SAFE_NO_PAD.decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap(),
        );

        assert_eq!(
            web_push_native::decrypt(ciphertext, &secret, &auth).unwrap(),
            b"When I grow up, I want to be a watermelon"
        );
    }

    fn keys() -> (String, String) {
        let private_key = URL_SAFE_NO_PAD.encode([7u8; 32]);
        let public_key = URL_SAFE_NO_PAD.encode(
            web_push_native::p256::SecretKey::from_slice(&[7; 32])
                .unwrap()
                .public_key()
                .to_encoded_point(false)
                .as_bytes(),
        );
        (private_key, public_key)
    }

    #[test]
    fn enabled_configuration_requires_matching_keys_contact_and_exact_provider_hosts() {
        let (private_key, public_key) = keys();
        assert!(
            WebPushConfig::new(
                private_key.clone(),
                public_key.clone(),
                "mailto:operator@example.test".into(),
                vec!["push.example.test".into()]
            )
            .is_ok()
        );
        assert!(
            WebPushConfig::new(
                private_key.clone(),
                URL_SAFE_NO_PAD.encode([0u8; 65]),
                "mailto:operator@example.test".into(),
                vec!["push.example.test".into()]
            )
            .is_err()
        );
        assert!(
            WebPushConfig::new(
                private_key.clone(),
                public_key.clone(),
                "invalid".into(),
                vec!["push.example.test".into()]
            )
            .is_err()
        );
        assert!(
            WebPushConfig::new(
                private_key,
                public_key,
                "mailto:operator@example.test".into(),
                vec!["*.example.test".into()]
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_private_key_lengths_are_redacted_and_rejected_before_the_upstream_builder() {
        let (_, public_key) = keys();
        for length in [0, 31, 33, 35, 64] {
            let private_key = URL_SAFE_NO_PAD.encode(vec![7u8; length]);
            let error = WebPushConfig::new(
                private_key.clone(),
                public_key.clone(),
                "mailto:operator@example.test".into(),
                vec!["push.example.test".into()],
            )
            .err()
            .unwrap();
            assert!(!error.to_string().contains(&private_key) || private_key.is_empty());
        }
    }

    #[test]
    fn absent_or_disabled_config_needs_no_push_secrets() {
        assert!(WebPushConfig::from_values(|_| None).unwrap().is_none());
        assert!(
            WebPushConfig::from_values(|name| (name == "WEB_PUSH_ENABLED").then(|| "false".into()))
                .unwrap()
                .is_none()
        );
        assert!(
            WebPushConfig::from_values(|name| (name == "WEB_PUSH_ENABLED").then(|| "true".into()))
                .is_err()
        );
    }
}

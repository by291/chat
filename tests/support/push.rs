//! Owned HTTPS push provider with real VAPID verification and RFC 8291 decryption.

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
    },
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;
use web_push_native::{
    Auth,
    jwt_simple::{
        algorithms::{ECDSAP256PublicKeyLike, ES256PublicKey},
        claims::NoCustomClaims,
        common::VerificationOptions,
    },
    p256::SecretKey,
};

const MAX_REQUEST_BYTES: usize = 16_384;
const READ_CHUNK_BYTES: usize = 1_024;

#[derive(Clone)]
pub struct CapturedPush {
    pub payload: serde_json::Value,
    pub vapid_valid: bool,
}

pub struct TestPushProvider {
    pub address: SocketAddr,
    pub certificate: Vec<u8>,
    captured: Arc<Mutex<Vec<CapturedPush>>>,
    status: Arc<AtomicU16>,
    silent: Arc<AtomicBool>,
    retry_after: Arc<Mutex<Option<String>>>,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl TestPushProvider {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["push.example.test".into()]).unwrap();
        let certificate = cert.der().to_vec();
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));

        let captured = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(AtomicU16::new(201));
        let silent = Arc::new(AtomicBool::new(false));
        let retry_after = Arc::new(Mutex::new(None::<String>));
        let cancel = CancellationToken::new();

        let task = {
            let captured = captured.clone();
            let status = status.clone();
            let silent = silent.clone();
            let retry_after = retry_after.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        accepted = listener.accept() => {
                            let (socket, _) = accepted.unwrap();
                            let captured = captured.clone();
                            let status = status.clone();
                            let silent = silent.clone();
                            let retry_after = retry_after.clone();
                            let acceptor = acceptor.clone();
                            let cancel = cancel.clone();
                            connections.spawn(async move {
                                // Connections are fixture-owned and may disappear during
                                // teardown. The capture forwards no durable work.
                                tokio::select! {
                                    _ = cancel.cancelled() => {},
                                    _ = capture_request(
                                        socket, acceptor, captured, status, silent,
                                        retry_after, cancel.clone(),
                                    ) => {},
                                }
                            });
                        },
                        _ = connections.join_next(), if !connections.is_empty() => {},
                    }
                }
            })
        };

        Self {
            address,
            certificate,
            captured,
            status,
            silent,
            retry_after,
            cancel,
            task,
        }
    }

    pub fn requests(&self) -> Vec<CapturedPush> {
        self.captured.lock().unwrap().clone()
    }

    pub fn respond(&self, status: u16, retry_after: Option<&str>) {
        self.status.store(status, Ordering::Relaxed);
        *self.retry_after.lock().unwrap() = retry_after.map(str::to_owned);
    }

    pub fn set_silent(&self) {
        self.silent.store(true, Ordering::Relaxed);
    }
}

impl Drop for TestPushProvider {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

async fn capture_request(
    socket: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    captured: Arc<Mutex<Vec<CapturedPush>>>,
    status: Arc<AtomicU16>,
    silent: Arc<AtomicBool>,
    retry_after: Arc<Mutex<Option<String>>>,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let mut socket = acceptor.accept(socket).await?;
    let mut bytes = Vec::new();
    let body_start = loop {
        let mut chunk = [0; READ_CHUNK_BYTES];
        let length = socket.read(&mut chunk).await?;
        if length == 0 || bytes.len() > MAX_REQUEST_BYTES {
            return Ok(());
        }

        bytes.extend_from_slice(&chunk[..length]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = String::from_utf8_lossy(&bytes[..body_start]).to_string();
    let header = |name: &str| {
        headers.lines().find_map(|line| {
            let (field, value) = line.split_once(':')?;
            field
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let body_length: usize = header("content-length").unwrap().parse().unwrap();
    if body_length > MAX_REQUEST_BYTES {
        return Ok(());
    }

    while bytes.len() < body_start + body_length {
        let mut chunk = [0; READ_CHUNK_BYTES];
        let length = socket.read(&mut chunk).await?;
        if length == 0 {
            return Ok(());
        }

        bytes.extend_from_slice(&chunk[..length]);
    }

    assert!(
        headers
            .to_ascii_lowercase()
            .contains("content-encoding: aes128gcm")
    );
    let authorization = header("authorization").unwrap();
    let (token, public_key) = authorization
        .strip_prefix("vapid t=")
        .unwrap()
        .split_once(", k=")
        .unwrap();
    let public_key =
        ES256PublicKey::from_bytes(&URL_SAFE_NO_PAD.decode(public_key).unwrap()).unwrap();
    let options = VerificationOptions {
        allowed_audiences: Some(
            ["https://push.example.test".to_owned()]
                .into_iter()
                .collect(),
        ),
        required_subject: Some("mailto:operator@example.test".into()),
        ..Default::default()
    };
    let vapid_valid = public_key
        .verify_token::<NoCustomClaims>(token, Some(options))
        .is_ok();
    let payload = web_push_native::decrypt(
        bytes[body_start..body_start + body_length].to_vec(),
        &SecretKey::from_slice(&[8; 32]).unwrap(),
        &Auth::clone_from_slice(&[9; 16]),
    )
    .unwrap();

    captured.lock().unwrap().push(CapturedPush {
        payload: serde_json::from_slice(&payload).unwrap(),
        vapid_valid,
    });
    if silent.load(Ordering::Relaxed) {
        cancel.cancelled().await;
        return Ok(());
    }

    let status = status.load(Ordering::Relaxed);
    let retry = retry_after
        .lock()
        .unwrap()
        .as_ref()
        .map(|value| format!("Retry-After: {value}\r\n"))
        .unwrap_or_default();
    let location = if status == 302 {
        "Location: https://127.0.0.1/private\r\n"
    } else {
        ""
    };
    let response = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n{retry}{location}\r\n"
    );
    socket.write_all(response.as_bytes()).await?;

    Ok(())
}

mod support;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use web_push_native::p256::{SecretKey, elliptic_curve::sec1::ToEncodedPoint};

struct PushFixture {
    app: support::TestApp,
    owner: reqwest::Client,
    user: chat_server::types::UserView,
}

impl PushFixture {
    async fn start() -> Self {
        let private_key = URL_SAFE_NO_PAD.encode([7u8; 32]);
        let public_key = URL_SAFE_NO_PAD.encode(
            SecretKey::from_slice(&[7; 32])
                .unwrap()
                .public_key()
                .to_encoded_point(false)
                .as_bytes(),
        );
        let mut config = chat_server::infrastructure::config::Config::from_env().unwrap();
        config.web_push = Some(
            chat_server::infrastructure::web_push::WebPushConfig::new(
                private_key,
                public_key,
                "mailto:operator@example.test".into(),
                vec!["push.example.test".into()],
            )
            .unwrap(),
        );
        let app = support::spawn_test_app_with_config(config).await;
        let (owner, user) = support::register_user(&app, "push_owner").await;

        Self { app, owner, user }
    }

    fn input(endpoint: &str, scalar: u8) -> Value {
        let public_key = SecretKey::from_slice(&[scalar; 32])
            .unwrap()
            .public_key()
            .to_encoded_point(false);
        json!({ "endpoint":format!("https://push.example.test/{endpoint}"), "p256dh":URL_SAFE_NO_PAD.encode(public_key.as_bytes()), "auth":URL_SAFE_NO_PAD.encode([scalar; 16]) })
    }

    async fn request(
        &self,
        client: &reqwest::Client,
        method: reqwest::Method,
        path: &str,
        body: Value,
    ) -> reqwest::Response {
        client
            .request(
                method,
                format!("{}/api/v1/notifications/{path}", self.app.base_url),
            )
            .header("Origin", &self.app.origin)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn register(&self, client: &reqwest::Client, input: Value) -> Value {
        let response = self
            .request(client, reqwest::Method::POST, "subscriptions", input)
            .await;
        assert_eq!(response.status(), 201);
        response.json().await.unwrap()
    }

    async fn login(&self) -> reqwest::Client {
        let client = self.app.client();
        let response = client.post(format!("{}/api/v1/auth/login", self.app.base_url))
            .header("Origin", &self.app.origin).json(&json!({"username":self.user.username,"password":"correct horse battery","device_label":"Push device"})).send().await.unwrap();
        assert_eq!(response.status(), 200);
        client
    }
}

#[tokio::test]
async fn disabled_push_keeps_auth_usable_and_config_requires_an_authenticated_owner() {
    let app = support::spawn_test_app().await;
    let (client, _) = support::register_user(&app, "push_owner").await;
    let url = format!("{}/api/v1/notifications/config", app.base_url);

    let response = client.get(&url).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["enabled"], false);
    assert!(body["vapid_public_key"].is_null());
    assert_eq!(app.client().get(&url).send().await.unwrap().status(), 401);
    assert_eq!(
        client
            .get(format!("{}/api/v1/auth/me", app.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn same_session_registration_is_stable_and_replacement_has_one_active_generation() {
    let fixture = PushFixture::start().await;
    let mut forged_identity = PushFixture::input("one", 7);
    forged_identity["user_id"] = json!(uuid::Uuid::new_v4());
    forged_identity["session_id"] = json!(uuid::Uuid::new_v4());
    let first = fixture.register(&fixture.owner, forged_identity).await;
    assert_eq!(
        sqlx::query_scalar::<_, chat_server::types::UserId>(
            "SELECT user_id FROM push_subscriptions WHERE id=$1"
        )
        .bind(uuid::Uuid::parse_str(first["id"].as_str().unwrap()).unwrap())
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        fixture.user.id
    );

    let repeated = fixture
        .register(&fixture.owner, PushFixture::input("one", 7))
        .await;
    let replaced = fixture
        .register(&fixture.owner, PushFixture::input("two", 8))
        .await;

    assert_eq!(first, repeated);
    assert_ne!(first["generation"], replaced["generation"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM push_subscriptions WHERE revoked_at IS NULL"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        1
    );
    let path = format!("subscriptions/{}", replaced["id"].as_str().unwrap());
    assert_eq!(
        fixture
            .request(&fixture.owner, reqwest::Method::DELETE, &path, Value::Null)
            .await
            .status(),
        204
    );
    assert_eq!(
        fixture
            .request(&fixture.owner, reqwest::Method::DELETE, &path, Value::Null)
            .await
            .status(),
        204
    );
}

#[tokio::test]
async fn foreign_capability_mismatch_preserves_victim_and_valid_rebind_changes_generation() {
    let fixture = PushFixture::start().await;
    let (other, _) = support::register_user(&fixture.app, "other_owner").await;
    let original = fixture
        .register(&fixture.owner, PushFixture::input("shared", 7))
        .await;

    let rejected = fixture
        .request(
            &other,
            reqwest::Method::POST,
            "subscriptions",
            PushFixture::input("shared", 8),
        )
        .await;

    assert_eq!(rejected.status(), 409);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM push_subscriptions WHERE revoked_at IS NULL"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        1
    );
    let rebound = fixture
        .register(&other, PushFixture::input("shared", 7))
        .await;
    assert_ne!(original["generation"], rebound["generation"]);
    let path = format!("subscriptions/{}", rebound["id"].as_str().unwrap());
    assert_eq!(
        fixture
            .request(&fixture.owner, reqwest::Method::DELETE, &path, Value::Null)
            .await
            .status(),
        404
    );
    assert_eq!(
        fixture
            .request(
                &fixture.owner,
                reqwest::Method::POST,
                &format!("{path}/activity"),
                json!({"visible":true,"focused":true,"conversation_id":null})
            )
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn ten_valid_devices_are_allowed_but_an_eleventh_is_rejected() {
    let fixture = PushFixture::start().await;
    fixture
        .register(&fixture.owner, PushFixture::input("device-0", 7))
        .await;
    for index in 1..10 {
        fixture
            .register(
                &fixture.login().await,
                PushFixture::input(&format!("device-{index}"), 7),
            )
            .await;
    }
    let eleventh = fixture.login().await;

    assert_eq!(
        fixture
            .request(
                &eleventh,
                reqwest::Method::POST,
                "subscriptions",
                PushFixture::input("device-10", 7)
            )
            .await
            .status(),
        409
    );

    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM push_subscriptions WHERE revoked_at IS NULL"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        10
    );
}

#[tokio::test]
async fn canonical_endpoint_aliases_cannot_bypass_the_existing_binding_capability() {
    let fixture = PushFixture::start().await;
    let (other, _) = support::register_user(&fixture.app, "alias_owner").await;
    let original = fixture
        .register(&fixture.owner, PushFixture::input("shared", 7))
        .await;
    let mut forged = PushFixture::input("shared", 8);
    forged["endpoint"] = json!("https://PUSH.EXAMPLE.TEST/shared");

    assert_eq!(
        fixture
            .request(&other, reqwest::Method::POST, "subscriptions", forged)
            .await
            .status(),
        409
    );

    let mut owned = PushFixture::input("shared", 7);
    owned["endpoint"] = json!("https://PUSH.EXAMPLE.TEST/shared");
    let rebound = fixture.register(&other, owned).await;
    assert_ne!(original["generation"], rebound["generation"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM push_subscriptions WHERE revoked_at IS NULL"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn two_accounts_can_swap_endpoint_capabilities_without_an_inverse_lock_cycle() {
    let fixture = PushFixture::start().await;
    let (other, _) = support::register_user(&fixture.app, "swap_owner").await;
    fixture
        .register(&fixture.owner, PushFixture::input("first", 7))
        .await;
    fixture
        .register(&other, PushFixture::input("second", 7))
        .await;

    let (owner, peer) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            fixture.register(&fixture.owner, PushFixture::input("second", 7)),
            fixture.register(&other, PushFixture::input("first", 7))
        )
    })
    .await
    .unwrap();

    assert_ne!(owner["id"], peer["id"]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM push_subscriptions WHERE revoked_at IS NULL"
        )
        .fetch_one(&fixture.app.pool)
        .await
        .unwrap(),
        2
    );
}

#[tokio::test]
async fn malformed_keys_and_unapproved_endpoints_never_create_a_binding() {
    let fixture = PushFixture::start().await;
    for (field, value) in [
        ("p256dh", "invalid"),
        ("auth", "invalid"),
        ("endpoint", "http://push.example.test/plain"),
        ("endpoint", "https://push.example.test:8080/token"),
        ("endpoint", "https://unapproved.example.test/token"),
        (
            "endpoint",
            "https://credential:secret@push.example.test/token",
        ),
    ] {
        let mut input = PushFixture::input("owned", 7);
        input[field] = json!(value);

        assert_eq!(
            fixture
                .request(
                    &fixture.owner,
                    reqwest::Method::POST,
                    "subscriptions",
                    input
                )
                .await
                .status(),
            400
        );
    }

    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM push_subscriptions")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn activity_uses_server_expiry_validates_membership_and_never_advances_reads() {
    let fixture = PushFixture::start().await;
    let (peer, peer_user) = support::register_user(&fixture.app, "push_peer").await;
    let actor = fixture
        .owner
        .post(format!(
            "{}/api/v1/conversations/direct",
            fixture.app.base_url
        ))
        .header("Origin", &fixture.app.origin)
        .json(&json!({"peer_id":peer_user.id}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let binding = fixture
        .register(&fixture.owner, PushFixture::input("activity", 7))
        .await;
    let path = format!("subscriptions/{}/activity", binding["id"].as_str().unwrap());

    assert_eq!(
        fixture
            .request(
                &fixture.owner,
                reqwest::Method::POST,
                &path,
                json!({"visible":true,"focused":true,"conversation_id":actor["id"]})
            )
            .await
            .status(),
        204
    );

    let lease: f64 = sqlx::query_scalar("SELECT extract(epoch FROM expires_at-clock_timestamp())::double precision FROM push_activity").fetch_one(&fixture.app.pool).await.unwrap();
    assert!((28.0..=30.0).contains(&lease));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(last_read_message_seq) FROM conversation_members")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .request(
                &peer,
                reqwest::Method::POST,
                &path,
                json!({"visible":true,"focused":true,"conversation_id":actor["id"]})
            )
            .await
            .status(),
        404
    );
    assert_eq!(
        fixture
            .request(
                &fixture.owner,
                reqwest::Method::POST,
                &path,
                json!({"visible":true,"focused":true,"conversation_id":uuid::Uuid::new_v4()})
            )
            .await
            .status(),
        404
    );
    assert_eq!(
        fixture
            .request(
                &fixture.owner,
                reqwest::Method::POST,
                &path,
                json!({"visible":false,"focused":false,"conversation_id":null})
            )
            .await
            .status(),
        204
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM push_activity")
            .fetch_one(&fixture.app.pool)
            .await
            .unwrap(),
        0
    );
}

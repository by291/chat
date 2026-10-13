#![allow(dead_code)]
use chat_server::{AppState, build_router, infrastructure::config::Config};
use sqlx::{PgPool, postgres::PgPoolOptions};

pub mod media;

pub struct TestApp {
    pub base_url: String,
    pub internal_url: String,
    pub pool: PgPool,
    pub state: AppState,
    pub origin: String,
    admin_url: String,
    database: String,
    servers: Vec<tokio::task::AbortHandle>,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        for server in &self.servers {
            server.abort();
        }
        let url = self.admin_url.clone();
        let database = self.database.clone();
        // The name is generated here, not supplied by a caller; cleanup survives a test runtime shutdown.
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    if let Ok(pool) = PgPool::connect(&url).await {
                        let sql = format!("DROP DATABASE {database} WITH (FORCE)");
                        let _ = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                            .execute(&pool)
                            .await;
                        pool.close().await;
                    }
                });
        });
    }
}

impl TestApp {
    pub fn client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .unwrap()
    }
}

pub async fn spawn_test_app() -> TestApp {
    spawn_test_app_with_config(Config::from_env().unwrap()).await
}

pub async fn spawn_test_app_with_config(mut config: Config) -> TestApp {
    let admin_url = config.database_url.clone();
    assert!(
        admin_url.contains("@127.0.0.1:55433/"),
        "Rust integration tests require the isolated test PostgreSQL; run node scripts/check.mjs"
    );
    let admin = PgPool::connect(&config.database_url).await.unwrap();
    let database = format!("test_{}", uuid::Uuid::new_v4().simple());
    // Identifier contains only test_ and a generated hexadecimal UUID, never user input.
    let create_sql = format!("CREATE DATABASE {database}");
    sqlx::query(sqlx::AssertSqlSafe(create_sql.as_str()))
        .execute(&admin)
        .await
        .unwrap();
    config.database_url = format!(
        "{}/{database}",
        config.database_url.rsplit_once('/').unwrap().0
    );
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    admin.close().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    config.allowed_origins = vec![base_url.clone()];
    let state = AppState::new(config, pool.clone()).await.unwrap();
    let router = build_router(state.clone());
    let internal = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let internal_url = format!("http://{}", internal.local_addr().unwrap());
    let internal_router = chat_server::realtime::http::build_internal_router(state.clone());
    let internal_task = tokio::spawn(async move {
        axum::serve(internal, internal_router).await.unwrap();
    });
    let public_task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    TestApp {
        origin: base_url.clone(),
        base_url,
        internal_url,
        pool,
        state,
        admin_url,
        database,
        servers: vec![internal_task.abort_handle(), public_task.abort_handle()],
    }
}

pub async fn register_user(
    app: &TestApp,
    name: &str,
) -> (reqwest::Client, chat_server::types::UserView) {
    use sha2::{Digest, Sha256};

    let token = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO invitations(id,token_hash,expires_at) VALUES($1,$2,now()+interval '7 days')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(hex::encode(Sha256::digest(token.as_bytes())))
    .execute(&app.pool)
    .await
    .unwrap();
    let client = app.client();
    let response = client
        .post(format!("{}/api/v1/auth/register", app.base_url))
        .header("Origin", &app.origin)
        .json(&serde_json::json!({
            "invitation": token,
            "username": name,
            "display_name": name,
            "password": "correct horse battery"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 201);
    let user = response.json().await.unwrap();
    (client, user)
}

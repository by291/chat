#[test]
fn forwarded_ip_requires_the_exact_trusted_proxy() {
    use chat_server::infrastructure::config::Config;
    let mut config = Config::from_env().unwrap();
    config.trusted_proxy_ips = vec!["172.30.240.10".parse().unwrap()];
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.9".parse().unwrap());
    assert_eq!(
        config
            .client_ip("172.30.240.10".parse().unwrap(), &headers)
            .to_string(),
        "203.0.113.9"
    );
    assert_eq!(
        config
            .client_ip("127.0.0.1".parse().unwrap(), &headers)
            .to_string(),
        "127.0.0.1"
    );
    headers.insert("x-forwarded-for", "203.0.113.9, 127.0.0.1".parse().unwrap());
    assert_eq!(
        config
            .client_ip("172.30.240.10".parse().unwrap(), &headers)
            .to_string(),
        "172.30.240.10"
    );
}

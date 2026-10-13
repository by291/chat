use chat_server::auth::password::PasswordService;

#[tokio::test]
async fn six_character_passwords_hash_and_verify_with_unicode_scalar_length() {
    let service = PasswordService::default();

    for password in ["abc123", "猫狗狐狸马鱼"] {
        let hash = service.hash(password).await.unwrap();

        assert!(service.verify(password, &hash).await.unwrap());
        assert!(!service.verify("xyz789", &hash).await.unwrap());
    }
}

#[tokio::test]
async fn five_character_and_oversized_passwords_are_rejected() {
    let service = PasswordService::default();
    let hash = service.hash("existing valid password").await.unwrap();

    for password in ["12345".to_owned(), "猫狗狐狸马".to_owned(), "x".repeat(129)] {
        let error = service.hash(&password).await.unwrap_err();

        assert_eq!(error.code, "invalid_input");
        assert!(!service.verify(&password, &hash).await.unwrap());
    }
}

#[tokio::test]
async fn the_128_character_upper_bound_remains_valid() {
    let service = PasswordService::default();
    let password = "x".repeat(128);

    let hash = service.hash(&password).await.unwrap();

    assert!(service.verify(&password, &hash).await.unwrap());
}

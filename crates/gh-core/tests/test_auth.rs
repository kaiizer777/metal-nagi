use gh_core::{
    create_octocrab_client, exchange_installation_token, generate_app_jwt, GitHubAppConfig,
};
use secrecy::{ExposeSecret, SecretString};

#[tokio::test]
async fn test_jwt_generation_and_token_exchange() -> Result<(), Box<dyn std::error::Error>> {
    let default_key_path = r"C:\Users\bari2\Downloads\metal-nagi.2026-11-10.private-key.pem";
    let key_path = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH")
        .unwrap_or_else(|_| default_key_path.to_string());

    if !std::path::Path::new(&key_path).exists() {
        println!("Skipping live GitHub API test: key file not found");
        return Ok(());
    }

    let app_id: u64 = std::env::var("GITHUB_APP_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5699171);
    let installation_id: u64 = std::env::var("GITHUB_INSTALLATION_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180574327);

    let pem_content = std::fs::read_to_string(&key_path)?;
    let private_key = SecretString::from(pem_content);

    // 1. Generate JWT
    let jwt = generate_app_jwt(app_id, &private_key)?;
    assert!(!jwt.is_empty(), "JWT should not be empty");

    // 2. Exchange JWT for Installation Access Token
    let token_info = exchange_installation_token(&jwt, installation_id).await?;

    let token_str = token_info.token.expose_secret();
    assert!(!token_str.is_empty(), "Installation token should not be empty");
    assert!(token_str.starts_with("ghs_"), "Installation token must start with 'ghs_'");
    assert!(
        token_info.expires_at > chrono::Utc::now(),
        "Token expiration should be in the future"
    );

    // 3. Authenticate with Octocrab client using the exchanged installation token
    let client = create_octocrab_client(&token_info.token)?;

    // Query /installation/repositories using Octocrab
    let res: serde_json::Value = client
        .get("/installation/repositories", None::<&()>)
        .await?;

    assert!(
        res.get("repositories").is_some() || res.get("total_count").is_some(),
        "Response should contain installation repositories payload"
    );

    Ok(())
}

#[test]
fn test_generate_app_jwt_with_mock_key() -> Result<(), Box<dyn std::error::Error>> {
    const TEST_RSA_KEY: &str = concat!(
        "-----BEGIN PRIVATE KEY-----\n",
        "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDpbszNr3jyFHw8\n",
        "bSgINwDduYbOpkFUowwkeUcv53JMMKMbs0azP5uSbsU3IYH3TRwBgqlv/JPxWqGu\n",
        "X3A4fukckZoXWlciR3XItX63DLZZrJxUMbbsyTX+eADvOqrdMbr7B0LtRyuqV6Or\n",
        "pDPLFjSYFw2eKiVWuz+ptyGn25SnABMrKd7+LTFHTR14Bc39OdAjcn5W2CHIs6UZ\n",
        "X0gByqbjNs2i/DOh2wBrZgyG9QFuAloYd/9sHilu0O/5qUdDqLbp4zs57K74KCxV\n",
        "+sxyEfASZW8mMk86SeZdh4G1YG+3W5Jq8sMPht4pcNgzBDoqnRXZb+7i2B3W59fB\n",
        "qB9YvwmHAgMBAAECggEAZszeUbi0lTuTSNMzfYankX26UfhirgDOAKQORxjwu3/x\n",
        "3OmTOa5J929j17cgcsXZDws5TjGX/4LP0CiElcTCKUwv5Zfp5pU82EKhhsCI7631\n",
        "/JgWPGoSJro3gyM9iwLzGsjkXvVoU/CbiiPakVX6nH4Oh7h80BjPXRDit/caHH6z\n",
        "ERarf6kdKGFDm1ZNjk+hbUjtWSxti11GUWZvsJyIhKQZq7/tIvWAlx8p9/aXWXIe\n",
        "Wu7yxv8kbofJgpuBvEjFGi7gpiECeXNeA0XzHJtIG5rV2CtkaSDpH/CIf0l815P4\n",
        "FH6IAnD8PwfLe+mn6Ay7J4YA5WN+VfpbcKDZ5Lu6OQKBgQD6o+hLvD4O/FkL9CW4\n",
        "b7YBadHt/CfcjgWry6Kj7JKx219uPCgEPvEPmxZ9KgyZ9YYoSZLmJvfA+EWFMl4v\n",
        "+p1qKaiOgzlp4W4jEAh1J0uW+p0i9c63MSSZb382vS2XDp9xA0TYXnrjdTlb6P5p\n",
        "tzpexFqLQcRxOsGn1ZjMszgZ3wKBgQDubLFoIQID54bCfeuCd0NRAxgF7c/auIF+\n",
        "bmBDmqJI1RT8KDuWaEvxvVFmVFR+ujGpgxk4mWkrGWddZivzMOC/SxssriugEpcc\n",
        "cuU7cuYhru+J6Rcd0iq7KZRSwkmGkA90EHpYy2P9XWSUh2SED5fRVu7Bv8o/MMbY\n",
        "SGAtOTlVWQKBgAIy8/POKAE7p5BaSQZwPHmQsx3Sa4sc1+Hk/H1RswvKb/DoqDU5\n",
        "VCYd5zmb59Zl+BJcauSIDP5tbza29WqYWU84JFlzv0o1hyDZoCle+ePOORi4bWLg\n",
        "FIEgIE6bpE3B/BYiwDThx1Q9kA5FYg4NZH62XFECd7XCnTZawmdxXxmjAoGBAKFS\n",
        "+lcyvmJV7mD7QrdsaygNp9ubJeEgpr4dgRU9NEGDrEdSEb+boodXShSrSuDUq3W/\n",
        "NvmNFBQNhFs2HtbWY7wL3b9Fuk79RTICrAilngF8lKgEB5gt9KvfXnUA8S8TmuE1\n",
        "pRMfy/BLmRrG4cRZqKgE4lhGpVpMbr85eZof9iNBAoGAH7U2HvibAGsyXZsEZM7e\n",
        "ixKbptNIq6PgV3y/aZ/lVVlqa5o9LG4ltsYmEDRUC0UKhFRRSyBqL30ySYfKzN/T\n",
        "0Fpn5ZgouMVK3rXEI8Yl78H47V2kskpaaL5AuMcppqRKAj67fSuxpHaK23jtLRi/\n",
        "hplK7XuSWWiMvzDd7bMm6NM=\n",
        "-----END PRIVATE KEY-----\n"
    );

    let key = SecretString::from(TEST_RSA_KEY.to_string());
    let jwt = generate_app_jwt(5699171, &key)?;
    assert!(!jwt.is_empty(), "JWT should not be empty");
    assert_eq!(jwt.split('.').count(), 3, "JWT must consist of header.payload.signature");

    Ok(())
}

#[test]
fn test_config_builder() {
    let secret_key = SecretString::from("dummy-pem".to_string());
    let webhook_secret = SecretString::from("dummy-secret".to_string());
    let config = GitHubAppConfig::new(5699171, secret_key, webhook_secret);
    assert_eq!(config.app_id(), 5699171);
    assert_eq!(config.webhook_secret().expose_secret(), "dummy-secret");
}

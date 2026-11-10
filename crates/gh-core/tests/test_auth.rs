use gh_core::{
    create_octocrab_client, exchange_installation_token, generate_app_jwt, GitHubAppConfig,
};
use secrecy::{ExposeSecret, SecretString};

#[tokio::test]
async fn test_jwt_generation_and_token_exchange() {
    let default_key_path = r"C:\Users\bari2\Downloads\metal-nagi.2026-11-10.private-key.pem";
    let key_path = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH")
        .unwrap_or_else(|_| default_key_path.to_string());

    if !std::path::Path::new(&key_path).exists() {
        panic!("Private key file not found at {key_path}");
    }

    let app_id: u64 = std::env::var("GITHUB_APP_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5699171);
    let installation_id: u64 = std::env::var("GITHUB_INSTALLATION_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180574327);

    let pem_content = std::fs::read_to_string(&key_path)
        .unwrap_or_else(|e| panic!("Failed to read private key from {key_path}: {e}"));
    let private_key = SecretString::from(pem_content);

    // 1. Generate JWT
    let jwt = generate_app_jwt(app_id, &private_key)
        .expect("Failed to generate GitHub App JWT");
    assert!(!jwt.is_empty(), "JWT should not be empty");

    // 2. Exchange JWT for Installation Access Token
    let token_info = exchange_installation_token(&jwt, installation_id)
        .await
        .expect("Failed to exchange JWT for installation token");

    let token_str = token_info.token.expose_secret();
    assert!(!token_str.is_empty(), "Installation token should not be empty");
    assert!(token_str.starts_with("ghs_"), "Installation token must start with 'ghs_'");
    assert!(
        token_info.expires_at > chrono::Utc::now(),
        "Token expiration should be in the future"
    );

    // 3. Authenticate with Octocrab client using the exchanged installation token
    let client = create_octocrab_client(&token_info.token)
        .expect("Failed to create octocrab client");

    // Query /installation/repositories using Octocrab
    let res: serde_json::Value = client
        .get("/installation/repositories", None::<&()>)
        .await
        .expect("Failed to query GitHub API with installation token");

    assert!(
        res.get("repositories").is_some() || res.get("total_count").is_some(),
        "Response should contain installation repositories payload"
    );
}

#[test]
fn test_config_builder() {
    let secret_key = SecretString::from("dummy-pem".to_string());
    let webhook_secret = SecretString::from("dummy-secret".to_string());
    let config = GitHubAppConfig::new(5699171, secret_key, webhook_secret);
    assert_eq!(config.app_id(), 5699171);
    assert_eq!(config.webhook_secret().expose_secret(), "dummy-secret");
}

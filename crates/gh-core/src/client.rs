use octocrab::Octocrab;
use secrecy::{ExposeSecret, SecretString};

use crate::config::GitHubAppConfig;
use crate::error::GhCoreError;
use crate::jwt::generate_app_jwt;
use crate::token::exchange_installation_token;

pub fn create_octocrab_client(installation_token: &SecretString) -> Result<Octocrab, GhCoreError> {
    let client = Octocrab::builder()
        .personal_token(installation_token.expose_secret().to_string())
        .build()?;
    Ok(client)
}

pub async fn create_app_installation_client(
    config: &GitHubAppConfig,
    installation_id: u64,
) -> Result<Octocrab, GhCoreError> {
    let jwt = generate_app_jwt(config.app_id(), config.private_key_pem())?;
    let token_info = exchange_installation_token(&jwt, installation_id).await?;
    create_octocrab_client(&token_info.token)
}

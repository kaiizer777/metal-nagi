use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use secrecy::SecretString;
use serde::Deserialize;

use crate::error::GhCoreError;

#[derive(Debug, Clone)]
pub struct InstallationToken {
    pub token: SecretString,
    pub expires_at: DateTime<Utc>,
    pub permissions: Option<serde_json::Value>,
    pub repository_selection: Option<String>,
}

#[derive(Deserialize)]
struct InstallationTokenResponse {
    token: String,
    expires_at: DateTime<Utc>,
    permissions: Option<serde_json::Value>,
    repository_selection: Option<String>,
}

pub async fn exchange_installation_token(
    jwt: &str,
    installation_id: u64,
) -> Result<InstallationToken, GhCoreError> {
    let client = reqwest::Client::new();
    exchange_installation_token_with_client(&client, jwt, installation_id).await
}

pub async fn exchange_installation_token_with_client(
    client: &reqwest::Client,
    jwt: &str,
    installation_id: u64,
) -> Result<InstallationToken, GhCoreError> {
    let url = format!("https://api.github.com/app/installations/{installation_id}/access_tokens");

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {jwt}"))
            .map_err(|e| GhCoreError::InvalidHeader(e.to_string()))?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("metal-nagi/0.1.0"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );

    let response = client
        .post(&url)
        .headers(headers)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(GhCoreError::TokenExchangeFailed {
            status: status.as_u16(),
            message: error_body,
        });
    }

    let parsed: InstallationTokenResponse = response.json().await?;
    Ok(InstallationToken {
        token: SecretString::from(parsed.token),
        expires_at: parsed.expires_at,
        permissions: parsed.permissions,
        repository_selection: parsed.repository_selection,
    })
}

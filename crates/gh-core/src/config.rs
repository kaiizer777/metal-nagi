use std::path::Path;
use secrecy::SecretString;

use crate::error::GhCoreError;

#[derive(Clone, Debug)]
pub struct GitHubAppConfig {
    app_id: u64,
    private_key_pem: SecretString,
    webhook_secret: SecretString,
}

impl GitHubAppConfig {
    pub fn new(app_id: u64, private_key_pem: SecretString, webhook_secret: SecretString) -> Self {
        Self {
            app_id,
            private_key_pem,
            webhook_secret,
        }
    }

    pub fn from_pem_file<P: AsRef<Path>>(
        app_id: u64,
        pem_path: P,
        webhook_secret: SecretString,
    ) -> Result<Self, GhCoreError> {
        let pem_content = std::fs::read_to_string(pem_path.as_ref())?;
        Ok(Self {
            app_id,
            private_key_pem: SecretString::from(pem_content),
            webhook_secret,
        })
    }

    pub fn from_env() -> Result<Self, GhCoreError> {
        let app_id_str = std::env::var("GITHUB_APP_ID")
            .map_err(|_| GhCoreError::Config("GITHUB_APP_ID environment variable not set".to_string()))?;
        let app_id = app_id_str.parse::<u64>().map_err(|e| {
            GhCoreError::Config(format!("Invalid GITHUB_APP_ID '{app_id_str}': {e}"))
        })?;

        let private_key_pem = if let Ok(key_content) = std::env::var("GITHUB_APP_PRIVATE_KEY") {
            SecretString::from(key_content)
        } else if let Ok(key_path) = std::env::var("GITHUB_APP_PRIVATE_KEY_PATH") {
            let pem_content = std::fs::read_to_string(&key_path).map_err(|e| {
                GhCoreError::Config(format!(
                    "Failed to read GITHUB_APP_PRIVATE_KEY_PATH at '{key_path}': {e}"
                ))
            })?;
            SecretString::from(pem_content)
        } else {
            return Err(GhCoreError::Config(
                "Neither GITHUB_APP_PRIVATE_KEY nor GITHUB_APP_PRIVATE_KEY_PATH is set".to_string(),
            ));
        };

        let webhook_secret = std::env::var("GITHUB_APP_WEBHOOK_SECRET")
            .map_err(|_| GhCoreError::Config("GITHUB_APP_WEBHOOK_SECRET is required".into()))?;
        if webhook_secret.trim().is_empty() {
            return Err(GhCoreError::Config("GITHUB_APP_WEBHOOK_SECRET is required".into()));
        }

        Ok(Self {
            app_id,
            private_key_pem,
            webhook_secret: SecretString::from(webhook_secret),
        })
    }

    pub fn app_id(&self) -> u64 {
        self.app_id
    }

    pub fn private_key_pem(&self) -> &SecretString {
        &self.private_key_pem
    }

    pub fn webhook_secret(&self) -> &SecretString {
        &self.webhook_secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_fails_when_webhook_secret_missing() {
        // SAFETY: Only in unit test for testing env var parsing
        unsafe {
            std::env::set_var("GITHUB_APP_ID", "12345");
            std::env::set_var("GITHUB_APP_PRIVATE_KEY", "dummy-key");
            std::env::remove_var("GITHUB_APP_WEBHOOK_SECRET");
        }

        let err = GitHubAppConfig::from_env().unwrap_err();
        match err {
            GhCoreError::Config(msg) => {
                assert_eq!(msg, "GITHUB_APP_WEBHOOK_SECRET is required");
            }
            other => panic!("Expected Config error, got {other:?}"),
        }
    }
}


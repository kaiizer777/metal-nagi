use std::path::{Path, PathBuf};
use std::time::Duration;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("GitHub core error: {0}")]
    GhCore(#[from] gh_core::GhCoreError),
    #[error("Git clone timed out after {0:?}")]
    Timeout(Duration),
    #[error("Git clone failed (exit code {code:?}): {stderr}")]
    CommandFailed {
        code: Option<i32>,
        stderr: String,
    },
    #[error("I/O error during git clone: {0}")]
    Io(#[from] std::io::Error),
}

pub fn sanitize_token(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "[REDACTED]")
    }
}

pub fn resolve_git_binary() -> &'static str {
    if Path::new("/opt/bin/git").exists() {
        "/opt/bin/git"
    } else {
        "git"
    }
}

pub fn resolve_target_path(job_id: &Uuid) -> PathBuf {
    std::env::temp_dir().join("repos").join(job_id.to_string())
}

pub async fn shallow_clone_repo_with_token(
    token: &SecretString,
    repo: &str,
    job_id: &Uuid,
) -> Result<PathBuf, GitError> {
    let target_path = resolve_target_path(job_id);

    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if target_path.exists() {
        tokio::fs::remove_dir_all(&target_path).await?;
    }

    let git_bin = resolve_git_binary();
    let clone_url = format!(
        "https://x-access-token:{}@github.com/{repo}.git",
        token.expose_secret()
    );

    let mut cmd = tokio::process::Command::new(git_bin);
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&clone_url)
        .arg(&target_path);

    let timeout_duration = Duration::from_secs(15);
    let output_res = tokio::time::timeout(timeout_duration, cmd.output()).await;

    let output = match output_res {
        Ok(Ok(output)) => output,
        Ok(Err(io_err)) => {
            let msg = sanitize_token(&io_err.to_string(), token.expose_secret());
            return Err(GitError::Io(std::io::Error::new(io_err.kind(), msg)));
        }
        Err(_) => return Err(GitError::Timeout(timeout_duration)),
    };

    if !output.status.success() {
        let raw_stderr = String::from_utf8_lossy(&output.stderr);
        let sanitized_stderr = sanitize_token(&raw_stderr, token.expose_secret());
        return Err(GitError::CommandFailed {
            code: output.status.code(),
            stderr: sanitized_stderr,
        });
    }

    Ok(target_path)
}

pub async fn shallow_clone_repo(
    client: &reqwest::Client,
    config: &gh_core::GitHubAppConfig,
    installation_id: u64,
    repo: &str,
    job_id: &Uuid,
) -> Result<PathBuf, GitError> {
    let jwt = gh_core::generate_app_jwt(config.app_id(), config.private_key_pem())?;
    let token_info =
        gh_core::exchange_installation_token_with_client(client, &jwt, installation_id).await?;

    shallow_clone_repo_with_token(&token_info.token, repo, job_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_token_redaction() {
        let token = "ghs_12345secrettoken67890";
        let raw_err = format!("fatal: unable to access 'https://x-access-token:{token}@github.com/foo/bar.git': 403");
        let sanitized = sanitize_token(&raw_err, token);

        assert!(!sanitized.contains(token));
        assert!(sanitized.contains("[REDACTED]"));
        assert_eq!(
            sanitized,
            "fatal: unable to access 'https://x-access-token:[REDACTED]@github.com/foo/bar.git': 403"
        );
    }

    #[test]
    fn test_sanitize_empty_token_is_noop() {
        let text = "error message without secrets";
        assert_eq!(sanitize_token(text, ""), text);
    }

    #[test]
    fn test_resolve_target_path() {
        let job_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let path = resolve_target_path(&job_id);
        assert!(path.ends_with(Path::new("repos").join("12345678-1234-1234-1234-123456789abc")));
    }

    #[test]
    fn test_resolve_git_binary() {
        let git_bin = resolve_git_binary();
        assert!(git_bin == "/opt/bin/git" || git_bin == "git");
    }
}

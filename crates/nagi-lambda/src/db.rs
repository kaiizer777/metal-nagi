use octocrab::Octocrab;
use sqlx::PgPool;
use uuid::Uuid;

use crate::webhook::PipelineType;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database query failed: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Active job conflict detected, but failed to fetch existing job")]
    ConflictMissingJob,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoConfigRow {
    pub owner_only: bool,
    pub allowed_senders: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockResult {
    Acquired { job_id: Uuid },
    AlreadyLocked { job_id: Uuid, current_phase: i32 },
}

pub fn evaluate_sender_authorization(
    config: Option<&RepoConfigRow>,
    repo_owner: &str,
    sender: &str,
) -> bool {
    if sender == repo_owner {
        return true;
    }

    match config {
        None => false,
        Some(cfg) => {
            if cfg.owner_only {
                false
            } else {
                cfg.allowed_senders
                    .as_array()
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some(sender)))
                    .unwrap_or(false)
            }
        }
    }
}

pub async fn is_sender_authorized(
    pool: &PgPool,
    repo: &str,
    repo_owner: &str,
    sender: &str,
) -> Result<bool, DbError> {
    let row: Option<(bool, serde_json::Value)> = sqlx::query_as(
        "SELECT owner_only, allowed_senders FROM repo_configs WHERE repo = $1",
    )
    .bind(repo)
    .fetch_optional(pool)
    .await?;

    let config_row = row.map(|(owner_only, allowed_senders)| RepoConfigRow {
        owner_only,
        allowed_senders,
    });

    Ok(evaluate_sender_authorization(
        config_row.as_ref(),
        repo_owner,
        sender,
    ))
}

pub async fn check_and_acquire_lock(
    pool: &PgPool,
    repo: &str,
    issue_or_pr_number: i64,
    pipeline_type: PipelineType,
) -> Result<LockResult, DbError> {
    let issue_num_i32 = issue_or_pr_number as i32;

    // 1. Check if active job lock already exists
    let active_job: Option<(Uuid, i32, String)> = sqlx::query_as(
        "SELECT job_id, current_phase, status FROM jobs WHERE repo = $1 AND issue_or_pr_number = $2 AND status = 'active'",
    )
    .bind(repo)
    .bind(issue_num_i32)
    .fetch_optional(pool)
    .await?;

    if let Some((job_id, current_phase, _)) = active_job {
        return Ok(LockResult::AlreadyLocked {
            job_id,
            current_phase,
        });
    }

    // 2. Try to insert active job
    let insert_result: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO jobs (repo, issue_or_pr_number, pipeline_type, status, current_phase) VALUES ($1, $2, $3, 'active', 1) RETURNING job_id",
    )
    .bind(repo)
    .bind(issue_num_i32)
    .bind(pipeline_type.as_str())
    .fetch_one(pool)
    .await;

    match insert_result {
        Ok(job_id) => Ok(LockResult::Acquired { job_id }),
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23505") => {
            // Race condition: concurrent job inserted. Query active job again.
            let active_job: Option<(Uuid, i32, String)> = sqlx::query_as(
                "SELECT job_id, current_phase, status FROM jobs WHERE repo = $1 AND issue_or_pr_number = $2 AND status = 'active'",
            )
            .bind(repo)
            .bind(issue_num_i32)
            .fetch_optional(pool)
            .await?;

            if let Some((job_id, current_phase, _)) = active_job {
                Ok(LockResult::AlreadyLocked {
                    job_id,
                    current_phase,
                })
            } else {
                Err(DbError::ConflictMissingJob)
            }
        }
        Err(e) => Err(DbError::Sqlx(e)),
    }
}

pub fn format_locked_comment(job_id: &Uuid, current_phase: i32) -> String {
    format!("⚠️ Nagi job `{job_id}` is already in progress (Phase {current_phase}) on this issue/PR.")
}

pub async fn post_locked_comment(
    client: &Octocrab,
    owner: &str,
    repo_name: &str,
    issue_number: u64,
    job_id: &Uuid,
    current_phase: i32,
) -> Result<octocrab::models::issues::Comment, octocrab::Error> {
    let comment_body = format_locked_comment(job_id, current_phase);
    client
        .issues(owner, repo_name)
        .create_comment(issue_number, comment_body)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sender_authorization_default_no_config() {
        assert!(evaluate_sender_authorization(None, "saif", "saif"));
        assert!(!evaluate_sender_authorization(None, "saif", "stranger"));
    }

    #[test]
    fn test_sender_authorization_owner_only_true() {
        let cfg = RepoConfigRow {
            owner_only: true,
            allowed_senders: serde_json::json!(["alice", "bob"]),
        };
        assert!(evaluate_sender_authorization(Some(&cfg), "saif", "saif"));
        assert!(!evaluate_sender_authorization(Some(&cfg), "saif", "alice"));
        assert!(!evaluate_sender_authorization(Some(&cfg), "saif", "stranger"));
    }

    #[test]
    fn test_sender_authorization_allowed_senders() {
        let cfg = RepoConfigRow {
            owner_only: false,
            allowed_senders: serde_json::json!(["alice", "bob"]),
        };
        assert!(evaluate_sender_authorization(Some(&cfg), "saif", "saif"));
        assert!(evaluate_sender_authorization(Some(&cfg), "saif", "alice"));
        assert!(evaluate_sender_authorization(Some(&cfg), "saif", "bob"));
        assert!(!evaluate_sender_authorization(Some(&cfg), "saif", "charlie"));
    }

    #[test]
    fn test_format_locked_comment() {
        let job_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let comment = format_locked_comment(&job_id, 2);
        assert_eq!(
            comment,
            "⚠️ Nagi job `550e8400-e29b-41d4-a716-446655440000` is already in progress (Phase 2) on this issue/PR."
        );
    }
}

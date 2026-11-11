use std::sync::Arc;
use lambda_http::{run, service_fn, Body, Error, Request, Response};
use nagi_lambda::db::{self, post_locked_comment, LockResult};
use nagi_lambda::git::{cleanup_repo_dir, shallow_clone_repo};
use nagi_lambda::webhook::{
    contains_mention, parse_pipeline_type, IssueCommentWebhookEvent, PipelineType,
};
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, info, warn};

pub struct AppState {
    pub pool: PgPool,
    pub github_config: gh_core::GitHubAppConfig,
    pub http_client: reqwest::Client,
}

fn extract_body_bytes(body: &Body) -> &[u8] {
    match body {
        Body::Empty => &[],
        Body::Text(s) => s.as_bytes(),
        Body::Binary(b) => b.as_slice(),
    }
}

fn json_response(status: u16, value: serde_json::Value) -> Result<Response<Body>, Error> {
    let body = serde_json::to_string(&value)?;
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::Text(body))
        .map_err(Into::into)
}

pub async fn handler(state: Arc<AppState>, event: Request) -> Result<Response<Body>, Error> {
    // 1. Extract headers: x-github-event and x-hub-signature-256
    let github_event = event
        .headers()
        .get("x-github-event")
        .and_then(|v| v.to_str().ok());

    let signature_header = event
        .headers()
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok());

    // 2. Filter event type
    match github_event {
        Some("issue_comment") => {}
        _ => {
            info!(event = ?github_event, "ignoring non-issue_comment event");
            return json_response(200, json!({ "status": "ignored_event" }));
        }
    }

    // 3. Verify HMAC signature against raw payload bytes
    let sig = match signature_header {
        Some(s) => s,
        None => {
            warn!("missing x-hub-signature-256 header");
            return json_response(
                401,
                json!({ "status": "unauthorized", "message": "missing signature header" }),
            );
        }
    };

    let body_bytes = extract_body_bytes(event.body());
    if let Err(e) = gh_core::verify_webhook_signature(
        state.github_config.webhook_secret(),
        sig,
        body_bytes,
    ) {
        warn!(error = %e, "invalid webhook signature");
        return json_response(
            401,
            json!({ "status": "unauthorized", "message": "invalid webhook signature" }),
        );
    }

    // 4. Parse JSON payload
    let webhook_event: IssueCommentWebhookEvent = match serde_json::from_slice(body_bytes) {
        Ok(evt) => evt,
        Err(e) => {
            warn!(error = %e, "failed to parse webhook payload JSON");
            return json_response(
                400,
                json!({ "status": "invalid_payload", "message": e.to_string() }),
            );
        }
    };

    // 5. Check action == "created"
    if webhook_event.action != "created" {
        info!(action = %webhook_event.action, "ignoring non-created issue comment action");
        return json_response(200, json!({ "status": "ignored_action" }));
    }

    // 6. Check @metal-nagi mention
    if !contains_mention(&webhook_event.comment.body) {
        info!("comment body does not contain @metal-nagi mention");
        return json_response(200, json!({ "status": "no_mention" }));
    }

    // 7. Check sender authorization
    let is_auth = match db::is_sender_authorized(
        &state.pool,
        &webhook_event.repository.full_name,
        &webhook_event.repository.owner.login,
        &webhook_event.sender.login,
    )
    .await
    {
        Ok(auth) => auth,
        Err(e) => {
            error!(error = %e, "database authorization check failed");
            return json_response(
                500,
                json!({ "status": "error", "message": "database error during authorization" }),
            );
        }
    };

    if !is_auth {
        warn!(
            sender = %webhook_event.sender.login,
            repo = %webhook_event.repository.full_name,
            "unauthorized sender attempted to trigger pipeline"
        );
        return json_response(200, json!({ "status": "unauthorized_sender" }));
    }

    // 8. Determine pipeline type (A: Issue, B: PR)
    let pipeline_type = parse_pipeline_type(&webhook_event.issue);

    // 9. Acquire concurrency lock
    let lock_result = match db::check_and_acquire_lock(
        &state.pool,
        &webhook_event.repository.full_name,
        webhook_event.issue.number,
        pipeline_type,
    )
    .await
    {
        Ok(res) => res,
        Err(db::DbError::IssueNumberOverflow(num)) => {
            warn!(number = num, "issue or pr number exceeds 32-bit integer limit");
            return json_response(
                400,
                json!({ "status": "invalid_payload", "message": format!("issue number {num} exceeds maximum allowed value") }),
            );
        }
        Err(e) => {
            error!(error = %e, "failed to acquire concurrency lock");
            return json_response(
                500,
                json!({ "status": "error", "message": "failed to acquire job lock" }),
            );
        }
    };

    let job_id = match lock_result {
        LockResult::AlreadyLocked {
            job_id,
            current_phase,
        } => {
            info!(
                job_id = %job_id,
                phase = current_phase,
                "job already in progress; posting status comment"
            );
            if let Some(installation) = &webhook_event.installation {
                let post_comment_task = async {
                    let octo = gh_core::create_app_installation_client(
                        &state.github_config,
                        installation.id,
                    )
                    .await
                    .map_err(|e| format!("failed to create installation client: {e}"))?;

                    let issue_num_u64 = u64::try_from(webhook_event.issue.number).unwrap_or(0);
                    post_locked_comment(
                        &octo,
                        &webhook_event.repository.owner.login,
                        &webhook_event.repository.name,
                        issue_num_u64,
                        &job_id,
                        current_phase,
                    )
                    .await
                    .map_err(|e| format!("failed to post lock comment: {e}"))?;

                    Ok::<(), String>(())
                };

                match tokio::time::timeout(std::time::Duration::from_secs(10), post_comment_task).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        warn!(error = %e, "failed to post lock comment to GitHub");
                    }
                    Err(_) => {
                        warn!("timed out posting lock comment to GitHub");
                    }
                }
            }
            return json_response(
                200,
                json!({
                    "status": "already_locked",
                    "job_id": job_id,
                    "current_phase": current_phase,
                }),
            );
        }
        LockResult::Acquired { job_id } => job_id,
    };

    info!(
        job_id = %job_id,
        pipeline = %pipeline_type,
        "concurrency lock acquired, proceeding with clone"
    );

    // 10. Execute shallow clone
    if let Some(installation) = &webhook_event.installation {
        let repo_path = match shallow_clone_repo(
            &state.http_client,
            &state.github_config,
            installation.id,
            &webhook_event.repository.full_name,
            &job_id,
        )
        .await
        {
            Ok(path) => path,
            Err(e) => {
                error!(job_id = %job_id, error = %e, "git shallow clone failed");
                if let Err(status_err) = db::release_lock(&state.pool, job_id, "failed").await {
                    error!(job_id = %job_id, error = %status_err, "failed to release lock / update job status to failed");
                }
                return json_response(
                    500,
                    json!({
                        "status": "error",
                        "job_id": job_id,
                        "message": format!("failed to clone repository: {e}")
                    }),
                );
            }
        };

        info!(job_id = %job_id, path = %repo_path.display(), "shallow clone succeeded");

        match pipeline_type {
            PipelineType::A => {
                let phase_input = json!({
                    "issue_title": webhook_event.issue.title,
                    "issue_body": webhook_event.issue.body,
                    "comment_body": webhook_event.comment.body,
                    "repo": webhook_event.repository.full_name,
                });

                if let Err(e) = db::record_job_phase(
                    &state.pool,
                    job_id,
                    1,
                    "in_progress",
                    Some(&phase_input),
                    None,
                )
                .await
                {
                    warn!(job_id = %job_id, error = %e, "failed to record job_phase 1 in_progress");
                }

                let prompt = format!(
                    "Repository: {}\nIssue #{}: {}\nIssue Body:\n{}\n\nTrigger Comment:\n{}",
                    webhook_event.repository.full_name,
                    webhook_event.issue.number,
                    webhook_event.issue.title.as_deref().unwrap_or(""),
                    webhook_event.issue.body.as_deref().unwrap_or(""),
                    webhook_event.comment.body
                );

                let request = llm_client::LlmRequest::new(vec![
                    llm_client::ChatMessage::system(
                        "You are Nagi, an autonomous AI software engineer. Analyze the GitHub issue and create an execution plan.",
                    ),
                    llm_client::ChatMessage::user(prompt),
                ])
                .with_max_tokens(1024);

                let orchestrator_res =
                    llm_client::FallbackOrchestrator::from_db(&state.pool).await;

                let orchestrator = match orchestrator_res {
                    Ok(orch) => orch,
                    Err(e) => {
                        error!(job_id = %job_id, error = %e, "failed to initialize FallbackOrchestrator from db");
                        if let Err(cleanup_err) = cleanup_repo_dir(&repo_path).await {
                            warn!(job_id = %job_id, path = %repo_path.display(), error = %cleanup_err, "failed to cleanup clone directory");
                        }
                        if let Err(phase_err) = db::record_job_phase(
                            &state.pool,
                            job_id,
                            1,
                            "failed",
                            Some(&phase_input),
                            Some(&json!({ "error": e.to_string() })),
                        )
                        .await
                        {
                            error!(job_id = %job_id, error = %phase_err, "failed to record job_phase failed");
                        }
                        if let Err(status_err) = db::release_lock(&state.pool, job_id, "failed").await {
                            error!(job_id = %job_id, error = %status_err, "failed to release lock / update job status to failed");
                        }

                        return json_response(
                            500,
                            json!({
                                "status": "error",
                                "job_id": job_id,
                                "message": format!("failed to initialize FallbackOrchestrator: {e}")
                            }),
                        );
                    }
                };

                match orchestrator.execute(&request).await {
                    Ok(llm_res) => {
                        info!(
                            job_id = %job_id,
                            model = %llm_res.model_used,
                            attempts = llm_res.attempts_taken,
                            "pipeline A phase 1 planning completed"
                        );

                        let phase_output = json!({
                            "model_used": llm_res.model_used,
                            "provider_used": llm_res.provider_used,
                            "attempts_taken": llm_res.attempts_taken,
                            "content": llm_res.content,
                        });

                        if let Err(e) = db::record_job_phase(
                            &state.pool,
                            job_id,
                            1,
                            "completed",
                            Some(&phase_input),
                            Some(&phase_output),
                        )
                        .await
                        {
                            error!(job_id = %job_id, error = %e, "failed to record job_phase completed");
                        }

                        if let Err(status_err) =
                            db::update_job_status(&state.pool, job_id, "completed", 1).await
                        {
                            error!(job_id = %job_id, error = %status_err, "failed to update job status to completed");
                        }
                    }
                    Err(llm_client::LlmError::TotalExhaustion) => {
                        error!(job_id = %job_id, "total exhaustion encountered across all fallback models");

                        if let Err(e) = db::transition_to_needs_human(&state.pool, job_id, 1).await {
                            error!(job_id = %job_id, error = %e, "failed to transition job to needs_human");
                        }

                        if let Err(phase_err) = db::record_job_phase(
                            &state.pool,
                            job_id,
                            1,
                            "failed",
                            Some(&phase_input),
                            Some(&json!({ "error": "TotalExhaustion" })),
                        )
                        .await
                        {
                            error!(job_id = %job_id, error = %phase_err, "failed to record job_phase failed");
                        }

                        match gh_core::create_app_installation_client(
                            &state.github_config,
                            installation.id,
                        )
                        .await
                        {
                            Ok(octo) => {
                                let issue_num_u64 = u64::try_from(webhook_event.issue.number).unwrap_or(0);
                                if let Err(e) = db::post_exhaustion_comment(
                                    &octo,
                                    &webhook_event.repository.owner.login,
                                    &webhook_event.repository.name,
                                    issue_num_u64,
                                )
                                .await
                                {
                                    error!(job_id = %job_id, error = %e, "failed to post exhaustion comment to github");
                                }
                            }
                            Err(e) => {
                                error!(job_id = %job_id, error = %e, "failed to create installation octocrab client for exhaustion alert");
                            }
                        }

                        if let Err(cleanup_err) = cleanup_repo_dir(&repo_path).await {
                            warn!(job_id = %job_id, path = %repo_path.display(), error = %cleanup_err, "failed to cleanup clone directory");
                        }

                        return json_response(
                            200,
                            json!({
                                "status": "needs_human",
                                "job_id": job_id,
                                "reason": "llm_exhaustion"
                            }),
                        );
                    }
                    Err(other_err) => {
                        error!(job_id = %job_id, error = %other_err, "llm execution encountered unexpected error");
                        if let Err(phase_err) = db::record_job_phase(
                            &state.pool,
                            job_id,
                            1,
                            "failed",
                            Some(&phase_input),
                            Some(&json!({ "error": other_err.to_string() })),
                        )
                        .await
                        {
                            error!(job_id = %job_id, error = %phase_err, "failed to record job_phase failed");
                        }
                        if let Err(status_err) = db::release_lock(&state.pool, job_id, "failed").await {
                            error!(job_id = %job_id, error = %status_err, "failed to release lock / update job status to failed");
                        }
                        if let Err(cleanup_err) = cleanup_repo_dir(&repo_path).await {
                            warn!(job_id = %job_id, path = %repo_path.display(), error = %cleanup_err, "failed to cleanup clone directory");
                        }
                        return json_response(
                            500,
                            json!({
                                "status": "error",
                                "job_id": job_id,
                                "message": format!("llm execution error: {other_err}")
                            }),
                        );
                    }
                }
            }
            PipelineType::B => {
                if let Err(status_err) =
                    db::update_job_status(&state.pool, job_id, "completed", 1).await
                {
                    error!(job_id = %job_id, error = %status_err, "failed to update job status to completed");
                }
            }
        }

        if let Err(cleanup_err) = cleanup_repo_dir(&repo_path).await {
            warn!(job_id = %job_id, path = %repo_path.display(), error = %cleanup_err, "failed to cleanup clone directory");
        }
    } else {
        warn!(job_id = %job_id, "no installation id present in webhook event, skipping clone");
        if let Err(status_err) = db::release_lock(&state.pool, job_id, "failed").await {
            error!(job_id = %job_id, error = %status_err, "failed to release lock for missing installation");
        }
        return json_response(
            500,
            json!({
                "status": "error",
                "job_id": job_id,
                "message": "missing installation id in webhook event",
            }),
        );
    }

    // 11. Return 200 OK with job info
    json_response(
        200,
        json!({
            "status": "job_created",
            "job_id": job_id,
            "pipeline": pipeline_type,
        }),
    )
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    let github_config = gh_core::GitHubAppConfig::from_env()
        .expect("Failed to initialize GitHubAppConfig from environment");

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");

    let state = Arc::new(AppState {
        pool,
        github_config,
        http_client,
    });

    run(service_fn(move |event: Request| {
        let state = Arc::clone(&state);
        async move { handler(state, event).await }
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use lambda_http::http;
    use secrecy::{ExposeSecret, SecretString};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    fn create_test_state() -> Arc<AppState> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://dummy:dummy@localhost:5432/dummy")
            .expect("valid connection string");

        let github_config = gh_core::GitHubAppConfig::new(
            12345,
            SecretString::from("dummy-private-key"),
            SecretString::from("webhook-secret-key"),
        );

        let http_client = reqwest::Client::new();

        Arc::new(AppState {
            pool,
            github_config,
            http_client,
        })
    }

    fn compute_signature(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[tokio::test]
    async fn ignores_non_issue_comment_event() {
        let state = create_test_state();
        let req = http::Request::builder()
            .header("x-github-event", "push")
            .body(Body::Empty)
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "ignored_event");
    }

    #[tokio::test]
    async fn rejects_missing_signature_header() {
        let state = create_test_state();
        let req = http::Request::builder()
            .header("x-github-event", "issue_comment")
            .body(Body::Empty)
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 401);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "unauthorized");
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let state = create_test_state();
        let req = http::Request::builder()
            .header("x-github-event", "issue_comment")
            .header("x-hub-signature-256", "sha256=badsignature0000000000000000000000000000000000000000000000000000")
            .body(Body::Text("dummy body".to_string()))
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 401);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "unauthorized");
    }

    #[tokio::test]
    async fn rejects_malformed_json_payload() {
        let state = create_test_state();
        let raw_payload = b"not-a-valid-json";
        let sig = compute_signature(
            state.github_config.webhook_secret().expose_secret(),
            raw_payload,
        );

        let req = http::Request::builder()
            .header("x-github-event", "issue_comment")
            .header("x-hub-signature-256", sig)
            .body(Body::Binary(raw_payload.to_vec()))
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 400);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "invalid_payload");
    }

    #[tokio::test]
    async fn ignores_non_created_action() {
        let state = create_test_state();
        let payload = json!({
            "action": "edited",
            "issue": {
                "number": 1,
                "title": "Title",
                "pull_request": null
            },
            "comment": {
                "id": 10,
                "body": "@metal-nagi run",
                "user": { "login": "saif" }
            },
            "repository": {
                "name": "metal-nagi",
                "full_name": "saif/metal-nagi",
                "owner": { "login": "saif" }
            },
            "installation": { "id": 1 },
            "sender": { "login": "saif" }
        });

        let raw_payload = serde_json::to_vec(&payload).unwrap();
        let sig = compute_signature(
            state.github_config.webhook_secret().expose_secret(),
            &raw_payload,
        );

        let req = http::Request::builder()
            .header("x-github-event", "issue_comment")
            .header("x-hub-signature-256", sig)
            .body(Body::Binary(raw_payload))
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "ignored_action");
    }

    #[tokio::test]
    async fn ignores_comment_without_mention() {
        let state = create_test_state();
        let payload = json!({
            "action": "created",
            "issue": {
                "number": 1,
                "title": "Title",
                "pull_request": null
            },
            "comment": {
                "id": 10,
                "body": "just a normal comment without mentioning bot",
                "user": { "login": "saif" }
            },
            "repository": {
                "name": "metal-nagi",
                "full_name": "saif/metal-nagi",
                "owner": { "login": "saif" }
            },
            "installation": { "id": 1 },
            "sender": { "login": "saif" }
        });

        let raw_payload = serde_json::to_vec(&payload).unwrap();
        let sig = compute_signature(
            state.github_config.webhook_secret().expose_secret(),
            &raw_payload,
        );

        let req = http::Request::builder()
            .header("x-github-event", "issue_comment")
            .header("x-hub-signature-256", sig)
            .body(Body::Binary(raw_payload))
            .unwrap();

        let resp = handler(state, req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = extract_body_bytes(resp.body());
        let val: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
        assert_eq!(val["status"], "no_mention");
    }
}

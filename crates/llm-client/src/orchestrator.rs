use std::sync::Arc;
use std::time::Duration;
use secrecy::SecretString;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::client::{LlmCaller, OpenCodeClient};
use crate::types::{LlmError, LlmRequest, LlmResponse};

pub fn is_retryable_for_model(err: &LlmError) -> bool {
    match err {
        LlmError::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
        LlmError::ApiError { status, body } => {
            *status == 429 || (*status >= 500 && *status < 600) || body.contains("Malformed JSON")
        }
        LlmError::Json(_) => true,
        _ => false,
    }
}

pub struct FallbackOrchestrator {
    provider_name: String,
    models: Vec<String>,
    client: Arc<dyn LlmCaller>,
    backoff: Duration,
    max_attempts_per_model: u32,
}

impl FallbackOrchestrator {
    pub fn new(
        provider_name: impl Into<String>,
        models: Vec<String>,
        client: Arc<dyn LlmCaller>,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            models,
            client,
            backoff: Duration::from_secs(5),
            max_attempts_per_model: 3,
        }
    }

    pub fn with_backoff(mut self, backoff: Duration) -> Self {
        self.backoff = backoff;
        self
    }

    pub fn with_max_attempts_per_model(mut self, max: u32) -> Self {
        self.max_attempts_per_model = max;
        self
    }

    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    pub fn models(&self) -> &[String] {
        &self.models
    }

    pub async fn from_db(pool: &PgPool, provider_name: &str) -> Result<Self, LlmError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT p.api_key_env_var, pm.model_name
            FROM provider_models pm
            JOIN providers p ON pm.provider_id = p.id
            WHERE p.name = $1 AND p.is_enabled = true AND pm.is_enabled = true
            ORDER BY pm.priority ASC
            "#,
        )
        .bind(provider_name)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            warn!(provider = provider_name, "no enabled models found for provider");
            return Err(LlmError::TotalExhaustion);
        }

        let api_key_env = &rows[0].0;
        let api_key_val = std::env::var(api_key_env)
            .map_err(|_| LlmError::MissingApiKey(api_key_env.clone()))?;

        let models: Vec<String> = rows.into_iter().map(|(_, model)| model).collect();
        let client = Arc::new(OpenCodeClient::new(SecretString::new(api_key_val.into())));

        Ok(Self::new(provider_name, models, client))
    }

    pub async fn execute(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let mut total_attempts: u32 = 0;

        for model in &self.models {
            info!(provider = %self.provider_name, model = %model, "attempting model completion");

            for attempt in 1..=self.max_attempts_per_model {
                total_attempts += 1;

                match self.client.call(model, request).await {
                    Ok(content) => {
                        info!(
                            provider = %self.provider_name,
                            model = %model,
                            total_attempts,
                            attempt_on_model = attempt,
                            "model completion succeeded"
                        );
                        return Ok(LlmResponse {
                            content,
                            model_used: model.clone(),
                            provider_used: self.provider_name.clone(),
                            attempts_taken: total_attempts,
                        });
                    }
                    Err(err) => {
                        let retryable = is_retryable_for_model(&err);
                        warn!(
                            provider = %self.provider_name,
                            model = %model,
                            attempt,
                            retryable,
                            error = %err,
                            "attempt failed"
                        );

                        if attempt < self.max_attempts_per_model && retryable {
                            if !self.backoff.is_zero() {
                                tokio::time::sleep(self.backoff).await;
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        warn!(
            provider = %self.provider_name,
            total_attempts,
            "all configured models exhausted"
        );
        Err(LlmError::TotalExhaustion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use crate::types::ChatMessage;

    struct MockClient {
        responses: Vec<Result<String, LlmError>>,
        call_count: AtomicUsize,
    }

    impl MockClient {
        fn new(responses: Vec<Result<String, LlmError>>) -> Self {
            Self {
                responses,
                call_count: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl LlmCaller for MockClient {
        fn call<'a>(
            &'a self,
            _model: &'a str,
            _request: &'a LlmRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, LlmError>> + Send + 'a>>
        {
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            let res = if idx < self.responses.len() {
                match &self.responses[idx] {
                    Ok(s) => Ok(s.clone()),
                    Err(LlmError::ApiError { status, body }) => Err(LlmError::ApiError {
                        status: *status,
                        body: body.clone(),
                    }),
                    Err(_) => Err(LlmError::TotalExhaustion),
                }
            } else {
                Err(LlmError::ApiError {
                    status: 500,
                    body: "unexpected mock overflow".to_string(),
                })
            };
            Box::pin(async move { res })
        }
    }

    #[tokio::test]
    async fn completes_on_first_try() {
        let mock = Arc::new(MockClient::new(vec![Ok("Plan generated".to_string())]));
        let orchestrator = FallbackOrchestrator::new(
            "opencode_go",
            vec!["beam-free".to_string(), "iceman".to_string()],
            mock.clone(),
        )
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Plan generated");
        assert_eq!(res.model_used, "beam-free");
        assert_eq!(res.attempts_taken, 1);
        assert_eq!(mock.calls(), 1);
    }

    #[tokio::test]
    async fn retries_transient_error_and_succeeds_on_second_attempt() {
        let mock = Arc::new(MockClient::new(vec![
            Err(LlmError::ApiError {
                status: 429,
                body: "Rate limited".to_string(),
            }),
            Ok("Second attempt succeeded".to_string()),
        ]));
        let orchestrator = FallbackOrchestrator::new(
            "opencode_go",
            vec!["beam-free".to_string()],
            mock.clone(),
        )
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Second attempt succeeded");
        assert_eq!(res.model_used, "beam-free");
        assert_eq!(res.attempts_taken, 2);
        assert_eq!(mock.calls(), 2);
    }

    #[tokio::test]
    async fn falls_back_to_second_model_after_three_failures() {
        let mock = Arc::new(MockClient::new(vec![
            // Model 1: 3 failures
            Err(LlmError::ApiError {
                status: 500,
                body: "Internal error".to_string(),
            }),
            Err(LlmError::ApiError {
                status: 502,
                body: "Bad gateway".to_string(),
            }),
            Err(LlmError::ApiError {
                status: 503,
                body: "Service unavailable".to_string(),
            }),
            // Model 2: succeeds on attempt 1
            Ok("Fallback succeeded".to_string()),
        ]));

        let orchestrator = FallbackOrchestrator::new(
            "opencode_go",
            vec!["beam-free".to_string(), "iceman-contributor-free".to_string()],
            mock.clone(),
        )
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Fallback succeeded");
        assert_eq!(res.model_used, "iceman-contributor-free");
        assert_eq!(res.attempts_taken, 4);
        assert_eq!(mock.calls(), 4);
    }

    #[tokio::test]
    async fn returns_total_exhaustion_when_all_models_fail() {
        let mock = Arc::new(MockClient::new(vec![
            // Model 1: 3 fails
            Err(LlmError::ApiError { status: 500, body: "Fail 1".into() }),
            Err(LlmError::ApiError { status: 500, body: "Fail 2".into() }),
            Err(LlmError::ApiError { status: 500, body: "Fail 3".into() }),
            // Model 2: 3 fails
            Err(LlmError::ApiError { status: 429, body: "Fail 4".into() }),
            Err(LlmError::ApiError { status: 429, body: "Fail 5".into() }),
            Err(LlmError::ApiError { status: 429, body: "Fail 6".into() }),
        ]));

        let orchestrator = FallbackOrchestrator::new(
            "opencode_go",
            vec!["beam-free".to_string(), "iceman".to_string()],
            mock.clone(),
        )
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let err = orchestrator.execute(&req).await.expect_err("should exhaust");

        assert!(matches!(err, LlmError::TotalExhaustion));
        assert_eq!(mock.calls(), 6);
    }
}

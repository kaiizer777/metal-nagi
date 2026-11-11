use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use secrecy::SecretString;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::client::{LlmCaller, OpenAiCompatibleClient};
use crate::types::{LlmError, LlmRequest, LlmResponse};

pub fn is_retryable_for_model(err: &LlmError) -> bool {
    match err {
        LlmError::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
        LlmError::ApiError { status, .. } => {
            *status == 429 || (*status >= 500 && *status < 600)
        }
        LlmError::MalformedResponse(_) => true,
        LlmError::Json(_) => true,
        _ => false,
    }
}

#[derive(Clone)]
pub struct FallbackCandidate {
    pub provider_name: String,
    pub model_name: String,
    pub caller: Arc<dyn LlmCaller>,
}

impl FallbackCandidate {
    pub fn new(
        provider_name: impl Into<String>,
        model_name: impl Into<String>,
        caller: Arc<dyn LlmCaller>,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            model_name: model_name.into(),
            caller,
        }
    }
}

pub struct FallbackOrchestrator {
    candidates: Vec<FallbackCandidate>,
    backoff: Duration,
    max_attempts_per_model: u32,
    timeout: Duration,
}

impl FallbackOrchestrator {
    pub fn new(candidates: Vec<FallbackCandidate>) -> Self {
        Self {
            candidates,
            backoff: Duration::from_secs(5),
            max_attempts_per_model: 3,
            timeout: Duration::from_secs(90),
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

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn candidates(&self) -> &[FallbackCandidate] {
        &self.candidates
    }

    pub async fn from_db(pool: &PgPool) -> Result<Self, LlmError> {
        let rows: Vec<(String, Option<String>, String, String)> = sqlx::query_as(
            r#"
            SELECT p.name as provider_name, p.base_url, p.api_key_env_var, pm.model_name
            FROM providers p
            JOIN provider_models pm ON p.id = pm.provider_id
            WHERE p.is_enabled = true AND pm.is_enabled = true
            ORDER BY p.priority ASC, pm.priority ASC;
            "#,
        )
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            warn!("no enabled providers or models found in database");
            return Err(LlmError::NoModelsConfigured(
                "No enabled providers or models found in database".to_string(),
            ));
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .unwrap_or_default();

        let mut caller_cache: HashMap<(String, String), Arc<dyn LlmCaller>> = HashMap::new();
        let mut candidates = Vec::with_capacity(rows.len());
        let mut missing_keys = Vec::new();

        for (provider_name, base_url_opt, api_key_env_var, model_name) in rows {
            let base_url = match base_url_opt {
                Some(url) if !url.trim().is_empty() => url,
                _ => {
                    warn!(
                        provider = %provider_name,
                        "provider has missing or empty base_url in db; skipping"
                    );
                    continue;
                }
            };

            let api_key_val = match std::env::var(&api_key_env_var) {
                Ok(val) => val,
                Err(_) => {
                    warn!(
                        provider = %provider_name,
                        env_var = %api_key_env_var,
                        "missing api key env var for provider; skipping"
                    );
                    missing_keys.push(api_key_env_var);
                    continue;
                }
            };

            let caller_key = (provider_name.clone(), base_url.clone());
            let caller = caller_cache
                .entry(caller_key)
                .or_insert_with(|| {
                    Arc::new(OpenAiCompatibleClient::with_client(
                        base_url,
                        SecretString::new(api_key_val.into()),
                        http_client.clone(),
                    ))
                })
                .clone();

            candidates.push(FallbackCandidate::new(provider_name, model_name, caller));
        }

        if candidates.is_empty() {
            if let Some(first_missing) = missing_keys.into_iter().next() {
                return Err(LlmError::MissingApiKey(first_missing));
            }
            return Err(LlmError::TotalExhaustion);
        }

        Ok(Self::new(candidates))
    }

    pub async fn execute(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let work = async {
            let mut total_attempts: u32 = 0;

            for candidate in &self.candidates {
                info!(
                    provider = %candidate.provider_name,
                    model = %candidate.model_name,
                    "attempting model completion"
                );

                for attempt in 1..=self.max_attempts_per_model {
                    total_attempts += 1;

                    match candidate.caller.call(&candidate.model_name, request).await {
                        Ok(content) => {
                            info!(
                                provider = %candidate.provider_name,
                                model = %candidate.model_name,
                                total_attempts,
                                attempt_on_model = attempt,
                                "model completion succeeded"
                            );
                            return Ok(LlmResponse {
                                content,
                                model_used: candidate.model_name.clone(),
                                provider_used: candidate.provider_name.clone(),
                                attempts_taken: total_attempts,
                            });
                        }
                        Err(err) => {
                            let retryable = is_retryable_for_model(&err);
                            warn!(
                                provider = %candidate.provider_name,
                                model = %candidate.model_name,
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

            warn!(total_attempts, "all configured provider models exhausted");
            Err(LlmError::TotalExhaustion)
        };

        match tokio::time::timeout(self.timeout, work).await {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    timeout_secs = self.timeout.as_secs(),
                    "overall fallback execution timed out; aborting with total exhaustion"
                );
                Err(LlmError::TotalExhaustion)
            }
        }
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
                    Err(LlmError::MalformedResponse(msg)) => {
                        Err(LlmError::MalformedResponse(msg.clone()))
                    }
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
        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock.clone()),
            FallbackCandidate::new("test-provider-1", "test-model-2", mock.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Plan generated");
        assert_eq!(res.provider_used, "test-provider-1");
        assert_eq!(res.model_used, "test-model-1");
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
        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Second attempt succeeded");
        assert_eq!(res.provider_used, "test-provider-1");
        assert_eq!(res.model_used, "test-model-1");
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

        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock.clone()),
            FallbackCandidate::new("test-provider-1", "test-model-2", mock.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Fallback succeeded");
        assert_eq!(res.provider_used, "test-provider-1");
        assert_eq!(res.model_used, "test-model-2");
        assert_eq!(res.attempts_taken, 4);
        assert_eq!(mock.calls(), 4);
    }

    #[tokio::test]
    async fn falls_back_across_providers() {
        let mock_provider1 = Arc::new(MockClient::new(vec![
            Err(LlmError::ApiError { status: 500, body: "Down 1".into() }),
            Err(LlmError::ApiError { status: 500, body: "Down 2".into() }),
            Err(LlmError::ApiError { status: 500, body: "Down 3".into() }),
        ]));
        let mock_provider2 = Arc::new(MockClient::new(vec![
            Ok("Provider 2 output".into()),
        ]));

        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock_provider1.clone()),
            FallbackCandidate::new("test-provider-2", "test-model-2", mock_provider2.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Cross provider task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Provider 2 output");
        assert_eq!(res.provider_used, "test-provider-2");
        assert_eq!(res.model_used, "test-model-2");
        assert_eq!(res.attempts_taken, 4);
        assert_eq!(mock_provider1.calls(), 3);
        assert_eq!(mock_provider2.calls(), 1);
    }

    #[tokio::test]
    async fn returns_total_exhaustion_when_all_models_fail() {
        let mock = Arc::new(MockClient::new(vec![
            // Candidate 1: 3 fails
            Err(LlmError::ApiError { status: 500, body: "Fail 1".into() }),
            Err(LlmError::ApiError { status: 500, body: "Fail 2".into() }),
            Err(LlmError::ApiError { status: 500, body: "Fail 3".into() }),
            // Candidate 2: 3 fails
            Err(LlmError::ApiError { status: 429, body: "Fail 4".into() }),
            Err(LlmError::ApiError { status: 429, body: "Fail 5".into() }),
            Err(LlmError::ApiError { status: 429, body: "Fail 6".into() }),
        ]));

        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock.clone()),
            FallbackCandidate::new("test-provider-2", "test-model-2", mock.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let err = orchestrator.execute(&req).await.expect_err("should exhaust");

        assert!(matches!(err, LlmError::TotalExhaustion));
        assert_eq!(mock.calls(), 6);
    }

    #[tokio::test]
    async fn retries_malformed_response_and_succeeds_on_second_attempt() {
        let mock = Arc::new(MockClient::new(vec![
            Err(LlmError::MalformedResponse("Bad JSON from wire".to_string())),
            Ok("Recovered from malformed response".to_string()),
        ]));
        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", mock.clone()),
        ])
        .with_backoff(Duration::ZERO);

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let res = orchestrator.execute(&req).await.expect("should succeed");

        assert_eq!(res.content, "Recovered from malformed response");
        assert_eq!(res.attempts_taken, 2);
        assert_eq!(mock.calls(), 2);
    }

    struct SlowMockClient {
        delay: Duration,
    }

    impl LlmCaller for SlowMockClient {
        fn call<'a>(
            &'a self,
            _model: &'a str,
            _request: &'a LlmRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, LlmError>> + Send + 'a>>
        {
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                Ok("Late response".to_string())
            })
        }
    }

    #[tokio::test]
    async fn aborts_with_total_exhaustion_when_timeout_deadline_exceeded() {
        let slow_mock = Arc::new(SlowMockClient {
            delay: Duration::from_millis(150),
        });
        let orchestrator = FallbackOrchestrator::new(vec![
            FallbackCandidate::new("test-provider-1", "test-model-1", slow_mock),
        ])
        .with_timeout(Duration::from_millis(20));

        let req = LlmRequest::new(vec![ChatMessage::user("Plan task")]);
        let err = orchestrator.execute(&req).await.expect_err("should timeout");

        assert!(matches!(err, LlmError::TotalExhaustion));
    }

    #[test]
    fn is_retryable_for_model_classifies_correctly() {
        assert!(is_retryable_for_model(&LlmError::MalformedResponse(
            "bad wire format".into()
        )));
        assert!(is_retryable_for_model(&LlmError::ApiError {
            status: 429,
            body: "rate limited".into(),
        }));
        assert!(is_retryable_for_model(&LlmError::ApiError {
            status: 502,
            body: "bad gateway".into(),
        }));
        assert!(!is_retryable_for_model(&LlmError::ApiError {
            status: 400,
            body: "bad request".into(),
        }));
        assert!(!is_retryable_for_model(&LlmError::NoModelsConfigured(
            "no models".into()
        )));
        assert!(!is_retryable_for_model(&LlmError::MissingApiKey(
            "KEY".into()
        )));
        assert!(!is_retryable_for_model(&LlmError::TotalExhaustion));
    }
}

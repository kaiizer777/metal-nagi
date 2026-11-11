use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::types::{ChatMessage, LlmError, LlmRequest};

pub trait LlmCaller: Send + Sync {
    fn call<'a>(
        &'a self,
        model: &'a str,
        request: &'a LlmRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, LlmError>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct OpenAiCompatibleClient {
    client: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    model: Option<String>,
}

pub type GenericLlmClient = OpenAiCompatibleClient;

#[derive(Serialize)]
struct ChatCompletionRequestBody<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ChatCompletionResponseBody {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
}

impl OpenAiCompatibleClient {
    pub fn new(base_url: impl Into<String>, api_key: SecretString) -> Self {
        Self::with_client_and_model(base_url, api_key, None, None)
    }

    pub fn with_model(
        base_url: impl Into<String>,
        api_key: SecretString,
        model: impl Into<String>,
    ) -> Self {
        Self::with_client_and_model(base_url, api_key, None, Some(model.into()))
    }

    pub fn with_client(
        base_url: impl Into<String>,
        api_key: SecretString,
        client: reqwest::Client,
    ) -> Self {
        Self::with_client_and_model(base_url, api_key, Some(client), None)
    }

    pub fn with_client_and_model(
        base_url: impl Into<String>,
        api_key: SecretString,
        client: Option<reqwest::Client>,
        model: Option<String>,
    ) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        let client = client.unwrap_or_else(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(45))
                .build()
                .unwrap_or_default()
        });
        Self {
            client,
            base_url: base,
            api_key,
            model,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub async fn chat_completion(
        &self,
        model: &str,
        request: &LlmRequest,
    ) -> Result<String, LlmError> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = ChatCompletionRequestBody {
            model,
            messages: &request.messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

        let response = self
            .client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            )
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError {
                status: status.as_u16(),
                body,
            });
        }

        let body_text = response.text().await?;
        let parsed: ChatCompletionResponseBody =
            serde_json::from_str(&body_text).map_err(|e| {
                LlmError::MalformedResponse(format!("Malformed JSON response ({e}): {body_text}"))
            })?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| {
                LlmError::MalformedResponse("Empty choices in response".to_string())
            })?;

        if content.trim().is_empty() {
            return Err(LlmError::MalformedResponse(
                "Empty or whitespace-only response content from LLM".to_string(),
            ));
        }

        Ok(content)
    }
}

impl LlmCaller for OpenAiCompatibleClient {
    fn call<'a>(
        &'a self,
        model: &'a str,
        request: &'a LlmRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, LlmError>> + Send + 'a>>
    {
        Box::pin(self.chat_completion(model, request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_successful_chat_completion_payload() {
        let raw_json = r#"{
            "id": "chatcmpl-123",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello from mock provider!"
                    },
                    "finish_reason": "stop"
                }
            ],
            "model": "test-model-1"
        }"#;

        let parsed: ChatCompletionResponseBody = serde_json::from_str(raw_json).unwrap();
        assert_eq!(
            parsed.choices[0].message.content.as_deref(),
            Some("Hello from mock provider!")
        );
    }

    #[test]
    fn handles_base_url_trailing_slashes() {
        let client = OpenAiCompatibleClient::new(
            "https://mock-provider.example.com/v1///",
            SecretString::new("test-key".into()),
        );
        assert_eq!(
            client.base_url(),
            "https://mock-provider.example.com/v1"
        );
    }

    #[test]
    fn instantiates_with_optional_model() {
        let client = OpenAiCompatibleClient::with_model(
            "https://mock-provider.example.com/v1",
            SecretString::new("test-key".into()),
            "test-model-1",
        );
        assert_eq!(client.model(), Some("test-model-1"));
    }

    async fn spawn_mock_server(
        status_line: &str,
        body: &str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status_line.to_string();
        let body = body.to_string();

        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
            }
        });

        (format!("http://{}", addr), handle)
    }

    #[tokio::test]
    async fn wire_response_200_valid() {
        let body = r#"{"choices":[{"message":{"content":"Valid response content"}}]}"#;
        let (base_url, _handle) = spawn_mock_server("200 OK", body).await;
        let client = OpenAiCompatibleClient::new(base_url, SecretString::new("key".into()));
        let req = LlmRequest::new(vec![ChatMessage::user("ping")]);

        let result = client.chat_completion("test-model", &req).await.unwrap();
        assert_eq!(result, "Valid response content");
    }

    #[tokio::test]
    async fn wire_response_5xx_returns_api_error() {
        let body = r#"{"error":"Internal server error"}"#;
        let (base_url, _handle) = spawn_mock_server("500 Internal Server Error", body).await;
        let client = OpenAiCompatibleClient::new(base_url, SecretString::new("key".into()));
        let req = LlmRequest::new(vec![ChatMessage::user("ping")]);

        let err = client.chat_completion("test-model", &req).await.unwrap_err();
        match err {
            LlmError::ApiError { status, body: err_body } => {
                assert_eq!(status, 500);
                assert!(err_body.contains("Internal server error"));
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wire_response_malformed_json_returns_malformed_response() {
        let body = r#"{"choices": [{ broken json"#;
        let (base_url, _handle) = spawn_mock_server("200 OK", body).await;
        let client = OpenAiCompatibleClient::new(base_url, SecretString::new("key".into()));
        let req = LlmRequest::new(vec![ChatMessage::user("ping")]);

        let err = client.chat_completion("test-model", &req).await.unwrap_err();
        match err {
            LlmError::MalformedResponse(msg) => {
                assert!(msg.contains("Malformed JSON response"));
            }
            other => panic!("expected MalformedResponse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wire_response_whitespace_content_returns_malformed_response() {
        let body = r#"{"choices":[{"message":{"content":"   \n\t  "}}]}"#;
        let (base_url, _handle) = spawn_mock_server("200 OK", body).await;
        let client = OpenAiCompatibleClient::new(base_url, SecretString::new("key".into()));
        let req = LlmRequest::new(vec![ChatMessage::user("ping")]);

        let err = client.chat_completion("test-model", &req).await.unwrap_err();
        match err {
            LlmError::MalformedResponse(msg) => {
                assert_eq!(msg, "Empty or whitespace-only response content from LLM");
            }
            other => panic!("expected MalformedResponse, got {other:?}"),
        }
    }
}

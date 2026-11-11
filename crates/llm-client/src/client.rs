use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::types::{ChatMessage, LlmError, LlmRequest};

pub const DEFAULT_BASE_URL: &str = "https://opencode.ai/inference/go/openai/v1";

pub trait LlmCaller: Send + Sync {
    fn call<'a>(
        &'a self,
        model: &'a str,
        request: &'a LlmRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, LlmError>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct OpenCodeClient {
    client: reqwest::Client,
    base_url: String,
    api_key: SecretString,
}

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

impl OpenCodeClient {
    pub fn new(api_key: SecretString) -> Self {
        let base_url = std::env::var("OPENCODE_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(api_key, base_url)
    }

    pub fn with_base_url(api_key: SecretString, base_url: impl Into<String>) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base,
            api_key,
        }
    }

    pub fn with_client(
        api_key: SecretString,
        base_url: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        Self {
            client,
            base_url: base,
            api_key,
        }
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
            serde_json::from_str(&body_text).map_err(|e| LlmError::ApiError {
                status: 200,
                body: format!("Malformed JSON response ({e}): {body_text}"),
            })?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| LlmError::ApiError {
                status: 200,
                body: "Empty choices in response".to_string(),
            })?;

        Ok(content)
    }
}

impl LlmCaller for OpenCodeClient {
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
                        "content": "Hello from OpenCode!"
                    },
                    "finish_reason": "stop"
                }
            ],
            "model": "beam-free"
        }"#;

        let parsed: ChatCompletionResponseBody = serde_json::from_str(raw_json).unwrap();
        assert_eq!(
            parsed.choices[0].message.content.as_deref(),
            Some("Hello from OpenCode!")
        );
    }

    #[test]
    fn handles_base_url_trailing_slashes() {
        let client = OpenCodeClient::with_base_url(
            SecretString::new("test-key".into()),
            "https://opencode.ai/inference/go/openai/v1///",
        );
        assert_eq!(
            client.base_url,
            "https://opencode.ai/inference/go/openai/v1"
        );
    }
}

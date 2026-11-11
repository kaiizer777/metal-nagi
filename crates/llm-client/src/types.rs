use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl LlmRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub model_used: String,
    pub provider_used: String,
    pub attempts_taken: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error (status {status}): {body}")]
    ApiError {
        status: u16,
        body: String,
    },

    #[error("Database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("Missing API key in environment variable: {0}")]
    MissingApiKey(String),

    #[error("Missing base URL for provider: {0}")]
    MissingBaseUrl(String),

    #[error("No models configured: {0}")]
    NoModelsConfigured(String),

    #[error("Malformed response from LLM: {0}")]
    MalformedResponse(String),

    #[error("Total exhaustion: all configured models failed after retry attempts")]
    TotalExhaustion,

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_and_deserializes_chat_message() {
        let msg = ChatMessage::user("Hello Nagi");
        let json = serde_json::to_string(&msg).expect("serialization failed");
        assert_eq!(json, r#"{"role":"user","content":"Hello Nagi"}"#);

        let parsed: ChatMessage = serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(parsed, msg);
    }

    #[test]
    fn serializes_llm_request_with_optional_fields() {
        let req = LlmRequest::new(vec![
            ChatMessage::system("You are an assistant"),
            ChatMessage::user("Hi"),
        ])
        .with_temperature(0.7)
        .with_max_tokens(512);

        let json = serde_json::to_value(&req).expect("to_value failed");
        assert_eq!(json["messages"].as_array().unwrap().len(), 2);
        let temp = json["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 1e-5);
        assert_eq!(json["max_tokens"], 512);

        let req_minimal = LlmRequest::new(vec![ChatMessage::user("Hi")]);
        let json_minimal = serde_json::to_string(&req_minimal).expect("to_string failed");
        assert!(!json_minimal.contains("temperature"));
        assert!(!json_minimal.contains("max_tokens"));
    }

    #[test]
    fn formats_llm_errors() {
        let err = LlmError::ApiError {
            status: 429,
            body: "Rate limit exceeded".to_string(),
        };
        assert_eq!(err.to_string(), "API error (status 429): Rate limit exceeded");

        let err2 = LlmError::MissingApiKey("OPENCODE_API_KEY".to_string());
        assert_eq!(
            err2.to_string(),
            "Missing API key in environment variable: OPENCODE_API_KEY"
        );

        let err3 = LlmError::NoModelsConfigured("No enabled providers".to_string());
        assert_eq!(
            err3.to_string(),
            "No models configured: No enabled providers"
        );

        let err4 = LlmError::MalformedResponse("Invalid json".to_string());
        assert_eq!(
            err4.to_string(),
            "Malformed response from LLM: Invalid json"
        );

        let err5 = LlmError::TotalExhaustion;
        assert_eq!(
            err5.to_string(),
            "Total exhaustion: all configured models failed after retry attempts"
        );
    }
}

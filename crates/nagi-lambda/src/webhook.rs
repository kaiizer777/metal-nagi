use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PipelineType {
    #[serde(rename = "A")]
    A,
    #[serde(rename = "B")]
    B,
}

impl PipelineType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }
}

impl std::fmt::Display for PipelineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderPayload {
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryPayload {
    pub name: String,
    pub full_name: String,
    pub owner: SenderPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallationPayload {
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentPayload {
    pub id: u64,
    pub body: String,
    pub user: SenderPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuePayload {
    pub number: i64,
    pub title: Option<String>,
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCommentWebhookEvent {
    pub action: String,
    pub issue: IssuePayload,
    pub comment: CommentPayload,
    pub repository: RepositoryPayload,
    pub installation: Option<InstallationPayload>,
    pub sender: SenderPayload,
}

pub fn parse_pipeline_type(issue: &IssuePayload) -> PipelineType {
    if issue.pull_request.is_some() {
        PipelineType::B
    } else {
        PipelineType::A
    }
}

pub fn contains_mention(body: &str) -> bool {
    let bytes = body.as_bytes();
    let target_len = 5; // "@nagi".len()

    if bytes.len() < target_len {
        return false;
    }

    let mut i = 0;
    while i + target_len <= bytes.len() {
        if bytes[i] == b'@'
            && bytes[i + 1].to_ascii_lowercase() == b'n'
            && bytes[i + 2].to_ascii_lowercase() == b'a'
            && bytes[i + 3].to_ascii_lowercase() == b'g'
            && bytes[i + 4].to_ascii_lowercase() == b'i'
        {
            let valid_prefix = if i == 0 {
                true
            } else {
                let prev = bytes[i - 1];
                !prev.is_ascii_alphanumeric() && prev != b'_' && prev != b'-' && prev != b'.'
            };

            let valid_suffix = if i + target_len == bytes.len() {
                true
            } else {
                let next = bytes[i + target_len];
                !next.is_ascii_alphanumeric() && next != b'_' && next != b'-'
            };

            if valid_prefix && valid_suffix {
                return true;
            }
        }
        i += 1;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_at_nagi_case_insensitively() {
        assert!(contains_mention("@nagi help"));
        assert!(contains_mention("@NAGI please review"));
        assert!(contains_mention("Hey @nagi!"));
        assert!(contains_mention("@nagi, please check"));
        assert!(contains_mention("cc: @nagi:"));
        assert!(contains_mention("PING @NAGI"));

        assert!(!contains_mention("@nagi-bot"));
        assert!(!contains_mention("foo@nagi.com"));
        assert!(!contains_mention("@nagisomething"));
        assert!(!contains_mention("No bot mentioned here"));
        assert!(!contains_mention("@otherbot help"));
    }

    #[test]
    fn parses_pipeline_type_correctly() {
        let issue_only = IssuePayload {
            number: 42,
            title: Some("Bug report".to_string()),
            pull_request: None,
        };
        assert_eq!(parse_pipeline_type(&issue_only), PipelineType::A);

        let pr_issue = IssuePayload {
            number: 43,
            title: Some("New feature".to_string()),
            pull_request: Some(serde_json::json!({
                "url": "https://api.github.com/repos/owner/repo/pulls/43"
            })),
        };
        assert_eq!(parse_pipeline_type(&pr_issue), PipelineType::B);
    }

    #[test]
    fn deserializes_github_issue_comment_payload() {
        let json_data = r#"{
            "action": "created",
            "issue": {
                "number": 101,
                "title": "Test Issue",
                "pull_request": null
            },
            "comment": {
                "id": 999123,
                "body": "@nagi run pipeline",
                "user": {
                    "login": "saif"
                }
            },
            "repository": {
                "name": "metal-nagi",
                "full_name": "saif/metal-nagi",
                "owner": {
                    "login": "saif"
                }
            },
            "installation": {
                "id": 12345678
            },
            "sender": {
                "login": "saif"
            }
        }"#;

        let event: IssueCommentWebhookEvent =
            serde_json::from_str(json_data).expect("valid webhook json");
        assert_eq!(event.action, "created");
        assert_eq!(event.issue.number, 101);
        assert_eq!(event.issue.title.as_deref(), Some("Test Issue"));
        assert!(event.issue.pull_request.is_none());
        assert_eq!(event.comment.id, 999123);
        assert_eq!(event.comment.body, "@nagi run pipeline");
        assert_eq!(event.comment.user.login, "saif");
        assert_eq!(event.repository.name, "metal-nagi");
        assert_eq!(event.repository.full_name, "saif/metal-nagi");
        assert_eq!(event.repository.owner.login, "saif");
        assert_eq!(event.installation.as_ref().map(|i| i.id), Some(12345678));
        assert_eq!(event.sender.login, "saif");
        assert_eq!(parse_pipeline_type(&event.issue), PipelineType::A);
        assert!(contains_mention(&event.comment.body));
    }

    #[test]
    fn deserializes_pr_comment_payload() {
        let json_data = r#"{
            "action": "created",
            "issue": {
                "number": 202,
                "title": "PR Title",
                "pull_request": {
                    "url": "https://api.github.com/repos/saif/metal-nagi/pulls/202"
                }
            },
            "comment": {
                "id": 888,
                "body": "LGTM @NAGI",
                "user": {
                    "login": "reviewer"
                }
            },
            "repository": {
                "name": "metal-nagi",
                "full_name": "saif/metal-nagi",
                "owner": {
                    "login": "saif"
                }
            },
            "installation": null,
            "sender": {
                "login": "reviewer"
            }
        }"#;

        let event: IssueCommentWebhookEvent =
            serde_json::from_str(json_data).expect("valid webhook json");
        assert_eq!(parse_pipeline_type(&event.issue), PipelineType::B);
        assert!(contains_mention(&event.comment.body));
    }
}

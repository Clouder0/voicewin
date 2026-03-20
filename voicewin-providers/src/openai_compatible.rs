use crate::request::{Body, HttpRequest};
use serde_json::json;

#[derive(Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleChatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

impl std::fmt::Debug for OpenAiCompatibleChatConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleChatConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub fn build_chat_completions_request(
    cfg: &OpenAiCompatibleChatConfig,
    messages: &[ChatMessage],
) -> HttpRequest {
    let url = join_url(&cfg.base_url, "/chat/completions");

    let mut payload = json!({
        "model": cfg.model,
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        "temperature": 0.3,
    });
    if let Some(reasoning_effort) = cfg
        .reasoning_effort
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        payload["reasoning"] = json!({ "effort": reasoning_effort });
    }

    HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("Authorization".into(), format!("Bearer {}", cfg.api_key)),
        ],
        body: Body::Json(payload.to_string()),
    }
}

pub fn build_list_models_request(base_url: &str, api_key: &str) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: join_url(base_url, "/models"),
        headers: vec![("Authorization".into(), format!("Bearer {}", api_key))],
        body: Body::Empty,
    }
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{}/{}", base, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_handles_trailing_slash() {
        assert_eq!(
            join_url("https://api.example.com/", "/chat/completions"),
            "https://api.example.com/chat/completions"
        );
        assert_eq!(
            join_url("https://api.example.com", "chat/completions"),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn builds_authorized_json_request() {
        let cfg = OpenAiCompatibleChatConfig {
            base_url: "https://api.example.com/v1".into(),
            api_key: "k".into(),
            model: "gpt-4o-mini".into(),
            reasoning_effort: None,
        };
        let req = build_chat_completions_request(
            &cfg,
            &[ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
        );

        assert_eq!(req.method, "POST");
        assert!(req.url.ends_with("/chat/completions"));
        assert_eq!(req.header("authorization"), Some("Bearer k"));
        match req.body {
            Body::Json(s) => assert!(s.contains("\"model\"")),
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn includes_reasoning_when_configured() {
        let cfg = OpenAiCompatibleChatConfig {
            base_url: "https://api.example.com/v1".into(),
            api_key: "k".into(),
            model: "gpt-5.4".into(),
            reasoning_effort: Some("medium".into()),
        };
        let req = build_chat_completions_request(
            &cfg,
            &[ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
        );

        match req.body {
            Body::Json(s) => {
                assert!(s.contains("\"reasoning\""));
                assert!(s.contains("\"effort\":\"medium\""));
            }
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn builds_authorized_models_get_request() {
        let req = build_list_models_request("https://api.example.com/v1", "k");

        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://api.example.com/v1/models");
        assert_eq!(req.header("authorization"), Some("Bearer k"));
        assert!(matches!(req.body, Body::Empty));
    }
}

use crate::request::{Body, HttpRequest};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleChatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
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

    let payload = json!({
        "model": cfg.model,
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        "temperature": 0.3,
    });

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
}

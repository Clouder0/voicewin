use crate::openai_compatible::ChatMessage;
use crate::request::{Body, HttpRequest};
use crate::sse::SseTextEvent;
use serde_json::{Value, json};
use voicewin_core::context::ImageArtifact;

#[derive(Clone, PartialEq, Eq)]
pub struct GeminiGenerateContentConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

impl std::fmt::Debug for GeminiGenerateContentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiGenerateContentConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

pub fn build_list_models_request(base_url: &str, api_key: &str) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url: join_url(base_url, "/models"),
        headers: vec![("x-goog-api-key".into(), api_key.into())],
        body: Body::Empty,
    }
}

pub fn build_stream_generate_content_request(
    cfg: &GeminiGenerateContentConfig,
    messages: &[ChatMessage],
    attached_image: Option<&ImageArtifact>,
) -> HttpRequest {
    let url = format!(
        "{}?alt=sse",
        join_url(
            &cfg.base_url,
            &format!("/models/{}:streamGenerateContent", cfg.model)
        )
    );

    let instructions = collect_messages_by_role(messages, "system");
    let input = {
        let user_input = collect_messages_by_role(messages, "user");
        if user_input.is_empty() {
            collect_non_system_messages(messages)
        } else {
            user_input
        }
    };

    let mut payload = json!({
        "contents": [
            {
                "role": "user",
                "parts": build_user_parts(&input, attached_image),
            }
        ]
    });

    if !instructions.is_empty() {
        payload["system_instruction"] = json!({
            "parts": [
                {
                    "text": instructions,
                }
            ]
        });
    }

    if let Some(reasoning_effort) = cfg
        .reasoning_effort
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        payload["generationConfig"] = json!({
            "thinkingConfig": {
                "thinkingLevel": reasoning_effort,
            }
        });
    }

    HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "text/event-stream".into()),
            ("x-goog-api-key".into(), cfg.api_key.clone()),
        ],
        body: Body::Json(payload.to_string()),
    }
}

fn build_user_parts(input: &str, attached_image: Option<&ImageArtifact>) -> Vec<Value> {
    let mut parts = Vec::new();

    if !input.trim().is_empty() {
        parts.push(json!({ "text": input }));
    }

    if let Some(image) = attached_image
        .as_ref()
        .filter(|image| !image.data_url.trim().is_empty())
    {
        let parsed =
            crate::data_url::parse_base64_image_data_url(&image.data_url).unwrap_or_else(|err| {
                panic!("invalid Gemini image artifact passed to request builder: {err}")
            });
        parts.push(json!({
            "inline_data": {
                "mime_type": parsed.mime_type,
                "data": parsed.base64_data,
            }
        }));
    }

    parts
}

pub fn extract_generate_content_sse_text_event(payload: &Value) -> SseTextEvent {
    let mut chunks = Vec::new();
    let mut done = false;

    let Some(candidates) = payload.get("candidates").and_then(Value::as_array) else {
        return SseTextEvent::default();
    };

    for candidate in candidates {
        if candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .is_some()
        {
            done = true;
        }

        let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        for part in parts {
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    chunks.push(text.to_string());
                }
            }
        }
    }

    SseTextEvent {
        delta: if chunks.is_empty() {
            None
        } else {
            Some(chunks.join(""))
        },
        full_text: None,
        input_tokens: None,
        cached_input_tokens: None,
        done,
    }
}

fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{}/{}", base, path)
}

fn collect_messages_by_role(messages: &[ChatMessage], role: &str) -> String {
    messages
        .iter()
        .filter(|message| message.role == role)
        .map(|message| message.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_non_system_messages(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| message.content.trim())
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_stream_generate_content_request() {
        let cfg = GeminiGenerateContentConfig {
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            api_key: "gem-key".into(),
            model: "gemini-3-flash-preview".into(),
            reasoning_effort: Some("low".into()),
        };

        let req = build_stream_generate_content_request(
            &cfg,
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "Rewrite only.".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "hello world".into(),
                },
            ],
            None,
        );

        assert_eq!(req.method, "POST");
        assert_eq!(req.header("x-goog-api-key"), Some("gem-key"));
        assert_eq!(req.header("accept"), Some("text/event-stream"));
        assert!(req.url.contains(":streamGenerateContent?alt=sse"));

        match req.body {
            Body::Json(body) => {
                assert!(body.contains("\"system_instruction\""));
                assert!(body.contains("\"hello world\""));
                assert!(body.contains("\"thinkingLevel\":\"low\""));
            }
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn builds_stream_generate_content_request_with_inline_image_data() {
        let cfg = GeminiGenerateContentConfig {
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            api_key: "gem-key".into(),
            model: "gemini-3-flash-preview".into(),
            reasoning_effort: None,
        };

        let req = build_stream_generate_content_request(
            &cfg,
            &[ChatMessage {
                role: "user".into(),
                content: "read the screenshot".into(),
            }],
            Some(&ImageArtifact {
                data_url: "data:image/png;base64,SGVsbG8=".into(),
            }),
        );

        match req.body {
            Body::Json(body) => {
                assert!(body.contains("\"inline_data\""));
                assert!(body.contains("\"mime_type\":\"image/png\""));
                assert!(body.contains("\"data\":\"SGVsbG8=\""));
            }
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn extracts_text_and_ignores_thought_parts() {
        let payload = json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "thought": true, "text": "internal" },
                            { "text": "Hello" },
                            { "text": ", world" }
                        ]
                    }
                }
            ]
        });

        assert_eq!(
            extract_generate_content_sse_text_event(&payload),
            SseTextEvent {
                delta: Some("Hello, world".into()),
                full_text: None,
                input_tokens: None,
                cached_input_tokens: None,
                done: false,
            }
        );
    }

    #[test]
    fn marks_done_when_finish_reason_is_present() {
        let payload = json!({
            "candidates": [
                {
                    "finishReason": "STOP",
                    "content": {
                        "parts": [
                            { "text": "done" }
                        ]
                    }
                }
            ]
        });

        assert_eq!(
            extract_generate_content_sse_text_event(&payload),
            SseTextEvent {
                delta: Some("done".into()),
                full_text: None,
                input_tokens: None,
                cached_input_tokens: None,
                done: true,
            }
        );
    }
}

use crate::openai_compatible::ChatMessage;
use crate::request::{Body, HttpRequest};
use crate::sse::SseTextEvent;
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use voicewin_core::context::ImageArtifact;

#[derive(Clone, PartialEq, Eq)]
pub struct OpenAiResponsesConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

impl std::fmt::Debug for OpenAiResponsesConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiResponsesConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

pub fn build_responses_sse_request(
    cfg: &OpenAiResponsesConfig,
    messages: &[ChatMessage],
    attached_image: Option<&ImageArtifact>,
) -> HttpRequest {
    let url = join_url(&cfg.base_url, "/responses");
    let instructions = collect_messages_by_role(messages, "system");
    let input_text = {
        let user_input = collect_messages_by_role(messages, "user");
        if user_input.is_empty() {
            collect_non_system_messages(messages)
        } else {
            user_input
        }
    };

    let mut payload = json!({
        "model": cfg.model,
        "stream": true,
        "store": false,
    });

    if let Some(image) = attached_image
        .as_ref()
        .filter(|image| !image.data_url.trim().is_empty())
    {
        payload["input"] = json!([
            {
                "role": "user",
                "content": build_multimodal_user_content(&input_text, image),
            }
        ]);
    } else {
        payload["input"] = json!(input_text);
    }

    if !instructions.is_empty() {
        payload["instructions"] = json!(instructions);
    }
    if let Some(prompt_cache_key) = build_prompt_cache_key(cfg, &instructions, attached_image) {
        // Keep the routing key stable across transcript changes so repeated cleanup requests
        // with the same static prefix can reuse provider-side prompt-cache locality.
        payload["prompt_cache_key"] = json!(prompt_cache_key);
    }

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
            ("Accept".into(), "text/event-stream".into()),
            ("Authorization".into(), format!("Bearer {}", cfg.api_key)),
        ],
        body: Body::Json(payload.to_string()),
    }
}

fn build_multimodal_user_content(input_text: &str, image: &ImageArtifact) -> Vec<Value> {
    let mut content = Vec::new();

    if !input_text.trim().is_empty() {
        content.push(json!({
            "type": "input_text",
            "text": input_text,
        }));
    }

    content.push(json!({
        "type": "input_image",
        "image_url": image.data_url,
        "detail": "low",
    }));

    content
}

pub fn extract_responses_sse_text_event(payload: &Value) -> SseTextEvent {
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if event_type == "response.output_text.delta" {
        return SseTextEvent {
            delta: payload
                .get("delta")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            full_text: None,
            input_tokens: None,
            cached_input_tokens: None,
            done: false,
        };
    }

    if event_type == "response.output_text.done" {
        return SseTextEvent {
            delta: None,
            full_text: payload
                .get("text")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            input_tokens: None,
            cached_input_tokens: None,
            done: false,
        };
    }

    if matches!(
        event_type,
        "response.output_item.done" | "response.output_item.added"
    ) {
        return SseTextEvent {
            delta: None,
            full_text: payload.get("item").and_then(extract_message_text),
            input_tokens: None,
            cached_input_tokens: None,
            done: false,
        };
    }

    if matches!(event_type, "response.completed" | "response.done") {
        return SseTextEvent {
            delta: None,
            full_text: payload.get("response").and_then(extract_response_text),
            input_tokens: payload
                .get("response")
                .and_then(extract_response_input_tokens),
            cached_input_tokens: payload
                .get("response")
                .and_then(extract_response_cached_input_tokens),
            done: true,
        };
    }

    if matches!(event_type, "response.failed" | "response.cancelled") {
        return SseTextEvent {
            delta: None,
            full_text: None,
            input_tokens: payload
                .get("response")
                .and_then(extract_response_input_tokens),
            cached_input_tokens: payload
                .get("response")
                .and_then(extract_response_cached_input_tokens),
            done: true,
        };
    }

    SseTextEvent::default()
}

fn extract_response_text(response: &Value) -> Option<String> {
    let output = response.get("output")?.as_array()?;
    let mut chunks = Vec::new();
    for item in output {
        if let Some(text) = extract_message_text(item) {
            chunks.push(text);
        }
    }
    join_non_empty(chunks)
}

fn extract_response_cached_input_tokens(response: &Value) -> Option<u64> {
    response
        .get("usage")
        .and_then(|usage| usage.get("input_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
}

fn extract_response_input_tokens(response: &Value) -> Option<u64> {
    response
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_u64)
}

fn extract_message_text(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }

    let content = item.get("content")?.as_array()?;
    let mut chunks = Vec::new();
    for part in content {
        if part.get("type").and_then(Value::as_str) == Some("output_text") {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                chunks.push(text.to_string());
            }
        }
    }
    join_non_empty(chunks)
}

fn join_non_empty(chunks: Vec<String>) -> Option<String> {
    let chunks: Vec<String> = chunks
        .into_iter()
        .filter(|chunk| !chunk.trim().is_empty())
        .collect();
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join(""))
    }
}

fn build_prompt_cache_key(
    cfg: &OpenAiResponsesConfig,
    instructions: &str,
    attached_image: Option<&ImageArtifact>,
) -> Option<String> {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    "voicewin-responses-v1".hash(&mut hasher);
    cfg.model.hash(&mut hasher);
    instructions.hash(&mut hasher);
    attached_image
        .as_ref()
        .filter(|image| !image.data_url.trim().is_empty())
        .is_some()
        .hash(&mut hasher);

    Some(format!(
        "voicewin:responses:v1:{}:{:016x}",
        cfg.model.trim(),
        hasher.finish()
    ))
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
    fn builds_responses_sse_request_with_store_false_and_stream_true() {
        let cfg = OpenAiResponsesConfig {
            base_url: "https://api.example.com/v1".into(),
            api_key: "k".into(),
            model: "gpt-5.4".into(),
            reasoning_effort: Some("medium".into()),
        };

        let req = build_responses_sse_request(
            &cfg,
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "system".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "user".into(),
                },
            ],
            None,
        );

        assert_eq!(req.method, "POST");
        assert!(req.url.ends_with("/responses"));
        assert_eq!(req.header("authorization"), Some("Bearer k"));
        assert_eq!(req.header("accept"), Some("text/event-stream"));

        match req.body {
            Body::Json(body) => {
                assert!(body.contains("\"stream\":true"));
                assert!(body.contains("\"store\":false"));
                assert!(body.contains("\"reasoning\""));
                assert!(body.contains("\"effort\":\"medium\""));
                assert!(body.contains("\"instructions\":\"system\""));
                assert!(body.contains("\"prompt_cache_key\":\"voicewin:responses:v1:gpt-5.4:"));
                assert!(body.contains("\"input\":\"user\""));
            }
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn builds_responses_sse_request_with_image_input() {
        let cfg = OpenAiResponsesConfig {
            base_url: "https://api.example.com/v1".into(),
            api_key: "k".into(),
            model: "gpt-5.4".into(),
            reasoning_effort: None,
        };

        let req = build_responses_sse_request(
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
                assert!(body.contains("\"type\":\"input_text\""));
                assert!(body.contains("\"type\":\"input_image\""));
                assert!(body.contains("\"detail\":\"low\""));
                assert!(body.contains("data:image/png;base64,SGVsbG8="));
            }
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn prompt_cache_key_ignores_transcript_text_but_changes_with_image_usage() {
        let cfg = OpenAiResponsesConfig {
            base_url: "https://api.example.com/v1".into(),
            api_key: "k".into(),
            model: "gpt-5.4".into(),
            reasoning_effort: None,
        };

        let first = build_responses_sse_request(
            &cfg,
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "stable instructions".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "first transcript".into(),
                },
            ],
            None,
        );
        let second = build_responses_sse_request(
            &cfg,
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "stable instructions".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "second transcript".into(),
                },
            ],
            None,
        );
        let with_image = build_responses_sse_request(
            &cfg,
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "stable instructions".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: "second transcript".into(),
                },
            ],
            Some(&ImageArtifact {
                data_url: "data:image/png;base64,SGVsbG8=".into(),
            }),
        );

        let extract_key = |req: HttpRequest| match req.body {
            Body::Json(body) => serde_json::from_str::<Value>(&body)
                .expect("json")
                .get("prompt_cache_key")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .expect("prompt cache key"),
            _ => panic!("expected json"),
        };

        let first_key = extract_key(first);
        let second_key = extract_key(second);
        let image_key = extract_key(with_image);

        assert_eq!(first_key, second_key);
        assert_ne!(first_key, image_key);
    }

    #[test]
    fn extracts_delta_text() {
        let payload = json!({
            "type": "response.output_text.delta",
            "delta": "Hel"
        });

        assert_eq!(
            extract_responses_sse_text_event(&payload),
            SseTextEvent {
                delta: Some("Hel".into()),
                full_text: None,
                input_tokens: None,
                cached_input_tokens: None,
                done: false,
            }
        );
    }

    #[test]
    fn extracts_full_text_from_output_item_done_message() {
        let payload = json!({
            "type": "response.output_item.done",
            "item": {
                "type": "message",
                "content": [
                    { "type": "output_text", "text": "Hello" },
                    { "type": "output_text", "text": ", world." }
                ]
            }
        });

        assert_eq!(
            extract_responses_sse_text_event(&payload),
            SseTextEvent {
                delta: None,
                full_text: Some("Hello, world.".into()),
                input_tokens: None,
                cached_input_tokens: None,
                done: false,
            }
        );
    }

    #[test]
    fn extracts_cached_input_tokens_from_response_completed() {
        let payload = json!({
            "type": "response.completed",
            "response": {
                "usage": {
                    "input_tokens": 2048,
                    "input_tokens_details": {
                        "cached_tokens": 768
                    }
                },
                "output": [
                    {
                        "type": "message",
                        "content": [
                            { "type": "output_text", "text": "Hello" }
                        ]
                    }
                ]
            }
        });

        assert_eq!(
            extract_responses_sse_text_event(&payload),
            SseTextEvent {
                delta: None,
                full_text: Some("Hello".into()),
                input_tokens: Some(2048),
                cached_input_tokens: Some(768),
                done: true,
            }
        );
    }

    #[test]
    fn extracts_full_text_from_output_item_added_message() {
        let payload = json!({
            "type": "response.output_item.added",
            "item": {
                "type": "message",
                "content": [
                    { "type": "output_text", "text": "Hello" },
                    { "type": "output_text", "text": " again" }
                ]
            }
        });

        assert_eq!(
            extract_responses_sse_text_event(&payload),
            SseTextEvent {
                delta: None,
                full_text: Some("Hello again".into()),
                input_tokens: None,
                cached_input_tokens: None,
                done: false,
            }
        );
    }

    #[test]
    fn extracts_full_text_from_completed_response() {
        let payload = json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "type": "message",
                    "content": [
                        { "type": "output_text", "text": "Hello" },
                        { "type": "output_text", "text": "!" }
                    ]
                }]
            }
        });

        assert_eq!(
            extract_responses_sse_text_event(&payload),
            SseTextEvent {
                delta: None,
                full_text: Some("Hello!".into()),
                input_tokens: None,
                cached_input_tokens: None,
                done: true,
            }
        );
    }
}

use crate::context::{VisualCaptureScope, VisualContextMode};
use serde::{Deserialize, Serialize};

pub fn llm_supports_attached_images(provider_kind: &str, api_kind: &str) -> bool {
    match provider_kind.trim() {
        "gemini" => matches!(
            api_kind.trim(),
            "stream_generate_content_sse" | "gemini_stream_sse" | ""
        ),
        "openai_compatible" | "" => matches!(api_kind.trim(), "responses_sse" | "responses"),
        _ => false,
    }
}

pub fn screenshot_context_warning(provider_kind: &str, api_kind: &str) -> Option<String> {
    if llm_supports_attached_images(provider_kind, api_kind) {
        return None;
    }

    let provider_kind = provider_kind.trim();
    let api_kind = api_kind.trim();
    Some(format!(
        "Configured LLM API does not support screenshot context (provider={} api={}); continuing without screenshot context.",
        if provider_kind.is_empty() {
            "openai_compatible"
        } else {
            provider_kind
        },
        if api_kind.is_empty() {
            "default"
        } else {
            api_kind
        }
    ))
}

pub fn visual_context_capture_unavailable_warning(
    dispatch: VisualContextDispatch,
    capture_scope: VisualCaptureScope,
) -> Option<String> {
    if matches!(dispatch, VisualContextDispatch::Off) {
        return None;
    }

    Some(format!(
        "Visual context capture did not produce a screenshot (dispatch={} capture_scope={}); continuing without visual context.",
        visual_context_dispatch_label(dispatch),
        visual_capture_scope_label(capture_scope),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualContextDispatch {
    Off,
    Screenshot,
    Ocr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenOcrSource {
    Inline,
    Prepared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualContextRuntime {
    pub mode: VisualContextMode,
    pub capture_scope: VisualCaptureScope,
    #[serde(default)]
    pub capture_actual_scope: Option<VisualCaptureScope>,
    pub dispatch: VisualContextDispatch,
    #[serde(default)]
    pub screenshot_capture_elapsed_ms: Option<u64>,
    #[serde(default)]
    pub capture_fallback_reason: Option<String>,
    #[serde(default)]
    pub screen_ocr_source: Option<ScreenOcrSource>,
    #[serde(default)]
    pub screen_ocr_elapsed_ms: Option<u64>,
    #[serde(default)]
    pub screen_ocr_first_token_ms: Option<u64>,
    #[serde(default)]
    pub screen_ocr_text_chars: Option<u64>,
}

impl Default for VisualContextRuntime {
    fn default() -> Self {
        Self {
            mode: VisualContextMode::Off,
            capture_scope: VisualCaptureScope::Display,
            capture_actual_scope: None,
            dispatch: VisualContextDispatch::Off,
            screenshot_capture_elapsed_ms: None,
            capture_fallback_reason: None,
            screen_ocr_source: None,
            screen_ocr_elapsed_ms: None,
            screen_ocr_first_token_ms: None,
            screen_ocr_text_chars: None,
        }
    }
}

pub fn resolve_visual_context_dispatch(
    mode: VisualContextMode,
    provider_kind: &str,
    api_kind: &str,
) -> VisualContextDispatch {
    match mode {
        VisualContextMode::Off => VisualContextDispatch::Off,
        VisualContextMode::Screenshot => {
            if llm_supports_attached_images(provider_kind, api_kind) {
                VisualContextDispatch::Screenshot
            } else {
                VisualContextDispatch::Off
            }
        }
        VisualContextMode::Ocr => VisualContextDispatch::Ocr,
        VisualContextMode::Auto => {
            if llm_supports_attached_images(provider_kind, api_kind) {
                VisualContextDispatch::Screenshot
            } else {
                VisualContextDispatch::Ocr
            }
        }
    }
}

pub fn visual_capture_requested(mode: VisualContextMode) -> bool {
    mode.is_enabled()
}

pub fn ocr_sidecar_api_kind(provider_kind: &str, api_kind: &str) -> Option<&'static str> {
    match provider_kind.trim() {
        "gemini" => Some("stream_generate_content_sse"),
        "openai_compatible" | "" => match api_kind.trim() {
            "responses_sse" | "responses" | "chat_completions" | "chat" | "" => {
                Some("responses_sse")
            }
            _ => Some("responses_sse"),
        },
        _ => None,
    }
}

pub fn visual_capture_scope_label(scope: VisualCaptureScope) -> &'static str {
    match scope {
        VisualCaptureScope::Display => "display",
        VisualCaptureScope::ForegroundWindow => "foreground_window",
    }
}

pub fn visual_context_dispatch_label(dispatch: VisualContextDispatch) -> &'static str {
    match dispatch {
        VisualContextDispatch::Off => "off",
        VisualContextDispatch::Screenshot => "screenshot",
        VisualContextDispatch::Ocr => "ocr",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VisualContextDispatch, llm_supports_attached_images, ocr_sidecar_api_kind,
        resolve_visual_context_dispatch, screenshot_context_warning, visual_capture_requested,
        visual_capture_scope_label, visual_context_capture_unavailable_warning,
        visual_context_dispatch_label,
    };
    use crate::context::{VisualCaptureScope, VisualContextMode};

    #[test]
    fn supports_images_for_openai_responses() {
        assert!(llm_supports_attached_images(
            "openai_compatible",
            "responses_sse"
        ));
    }

    #[test]
    fn supports_images_for_gemini_native() {
        assert!(llm_supports_attached_images(
            "gemini",
            "stream_generate_content_sse"
        ));
    }

    #[test]
    fn rejects_images_for_chat_completions() {
        assert!(!llm_supports_attached_images(
            "openai_compatible",
            "chat_completions"
        ));
        assert!(
            screenshot_context_warning("openai_compatible", "chat_completions")
                .unwrap()
                .contains("does not support screenshot context")
        );
    }

    #[test]
    fn auto_dispatch_prefers_screenshot_when_images_are_supported() {
        assert_eq!(
            resolve_visual_context_dispatch(
                VisualContextMode::Auto,
                "openai_compatible",
                "responses_sse"
            ),
            VisualContextDispatch::Screenshot
        );
    }

    #[test]
    fn auto_dispatch_falls_back_to_ocr_for_text_only_api() {
        assert_eq!(
            resolve_visual_context_dispatch(
                VisualContextMode::Auto,
                "openai_compatible",
                "chat_completions"
            ),
            VisualContextDispatch::Ocr
        );
    }

    #[test]
    fn screenshot_dispatch_drops_to_off_when_images_are_not_supported() {
        assert_eq!(
            resolve_visual_context_dispatch(
                VisualContextMode::Screenshot,
                "openai_compatible",
                "chat_completions"
            ),
            VisualContextDispatch::Off
        );
    }

    #[test]
    fn ocr_sidecar_uses_responses_for_openai_compatible() {
        assert_eq!(
            ocr_sidecar_api_kind("openai_compatible", "chat_completions"),
            Some("responses_sse")
        );
    }

    #[test]
    fn capture_unavailable_warning_mentions_dispatch_and_scope() {
        assert_eq!(
            visual_context_capture_unavailable_warning(
                VisualContextDispatch::Ocr,
                VisualCaptureScope::ForegroundWindow
            )
            .as_deref(),
            Some(
                "Visual context capture did not produce a screenshot (dispatch=ocr capture_scope=foreground_window); continuing without visual context."
            )
        );
    }

    #[test]
    fn capture_unavailable_warning_is_absent_for_off_dispatch() {
        assert_eq!(
            visual_context_capture_unavailable_warning(
                VisualContextDispatch::Off,
                VisualCaptureScope::Display
            ),
            None
        );
    }

    #[test]
    fn dispatch_label_formats_expected_variants() {
        assert_eq!(
            visual_context_dispatch_label(VisualContextDispatch::Screenshot),
            "screenshot"
        );
        assert_eq!(
            visual_context_dispatch_label(VisualContextDispatch::Ocr),
            "ocr"
        );
    }

    #[test]
    fn visual_capture_requested_tracks_mode() {
        assert!(!visual_capture_requested(VisualContextMode::Off));
        assert!(visual_capture_requested(VisualContextMode::Screenshot));
        assert!(visual_capture_requested(VisualContextMode::Ocr));
    }

    #[test]
    fn visual_capture_scope_label_is_stable() {
        assert_eq!(
            visual_capture_scope_label(VisualCaptureScope::ForegroundWindow),
            "foreground_window"
        );
    }
}

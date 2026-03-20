use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub data_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScreenshotCaptureMetadata {
    #[serde(default)]
    pub actual_scope: Option<VisualCaptureScope>,
    #[serde(default)]
    pub capture_elapsed_ms: Option<u64>,
    #[serde(default)]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisualContextMode {
    #[default]
    Off,
    Auto,
    Screenshot,
    Ocr,
}

impl VisualContextMode {
    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisualCaptureScope {
    #[default]
    Display,
    ForegroundWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextToggles {
    pub use_clipboard: bool,
    pub use_selected_text: bool,
    pub use_window_context: bool,
    pub use_custom_vocabulary: bool,

    pub visual_context_mode: VisualContextMode,
    pub visual_capture_scope: VisualCaptureScope,
}

impl Default for ContextToggles {
    fn default() -> Self {
        Self {
            use_clipboard: true,
            use_selected_text: false,
            use_window_context: true,
            use_custom_vocabulary: true,
            visual_context_mode: VisualContextMode::Off,
            visual_capture_scope: VisualCaptureScope::Display,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ContextTogglesWire {
    #[serde(default)]
    use_clipboard: Option<bool>,
    #[serde(default)]
    use_selected_text: Option<bool>,
    #[serde(default)]
    use_window_context: Option<bool>,
    #[serde(default)]
    use_custom_vocabulary: Option<bool>,
    #[serde(default)]
    visual_context_mode: Option<VisualContextMode>,
    #[serde(default)]
    visual_capture_scope: Option<VisualCaptureScope>,
    #[serde(default)]
    use_ocr: Option<bool>,
}

impl<'de> Deserialize<'de> for ContextToggles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ContextTogglesWire::deserialize(deserializer)?;
        let defaults = Self::default();
        let visual_context_mode = match wire.visual_context_mode {
            Some(mode) => mode,
            None => match wire.use_ocr.unwrap_or(false) {
                true => VisualContextMode::Screenshot,
                false => VisualContextMode::Off,
            },
        };
        Ok(Self {
            use_clipboard: wire.use_clipboard.unwrap_or(defaults.use_clipboard),
            use_selected_text: wire.use_selected_text.unwrap_or(defaults.use_selected_text),
            use_window_context: wire
                .use_window_context
                .unwrap_or(defaults.use_window_context),
            use_custom_vocabulary: wire
                .use_custom_vocabulary
                .unwrap_or(defaults.use_custom_vocabulary),
            visual_context_mode,
            visual_capture_scope: wire
                .visual_capture_scope
                .unwrap_or(defaults.visual_capture_scope),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextToggles, ScreenshotCaptureMetadata, VisualCaptureScope, VisualContextMode};

    #[test]
    fn defaults_use_visual_context_off_and_display_scope() {
        let toggles = ContextToggles::default();
        assert_eq!(toggles.visual_context_mode, VisualContextMode::Off);
        assert_eq!(toggles.visual_capture_scope, VisualCaptureScope::Display);
    }

    #[test]
    fn deserializes_legacy_use_ocr_into_screenshot_mode() {
        let toggles: ContextToggles = serde_json::from_str(
            r#"{
                "use_clipboard": true,
                "use_selected_text": false,
                "use_window_context": true,
                "use_custom_vocabulary": true,
                "use_ocr": true
            }"#,
        )
        .unwrap();
        assert_eq!(toggles.visual_context_mode, VisualContextMode::Screenshot);
        assert_eq!(toggles.visual_capture_scope, VisualCaptureScope::Display);
    }

    #[test]
    fn prefers_new_visual_context_mode_over_legacy_flag() {
        let toggles: ContextToggles = serde_json::from_str(
            r#"{
                "visual_context_mode": "ocr",
                "visual_capture_scope": "foreground_window",
                "use_ocr": false
            }"#,
        )
        .unwrap();
        assert_eq!(toggles.visual_context_mode, VisualContextMode::Ocr);
        assert_eq!(
            toggles.visual_capture_scope,
            VisualCaptureScope::ForegroundWindow
        );
    }

    #[test]
    fn serializes_new_visual_fields_without_legacy_use_ocr() {
        let json = serde_json::to_value(ContextToggles {
            visual_context_mode: VisualContextMode::Auto,
            visual_capture_scope: VisualCaptureScope::ForegroundWindow,
            ..ContextToggles::default()
        })
        .unwrap();
        assert_eq!(
            json.get("visual_context_mode").and_then(|v| v.as_str()),
            Some("auto")
        );
        assert_eq!(
            json.get("visual_capture_scope").and_then(|v| v.as_str()),
            Some("foreground_window")
        );
        assert!(json.get("use_ocr").is_none());
    }

    #[test]
    fn screenshot_capture_metadata_round_trips_optional_fields() {
        let json = serde_json::to_value(ScreenshotCaptureMetadata {
            actual_scope: Some(VisualCaptureScope::Display),
            capture_elapsed_ms: Some(42),
            fallback_reason: Some("foreground_window_not_implemented".into()),
        })
        .unwrap();
        assert_eq!(
            json.get("actual_scope").and_then(|v| v.as_str()),
            Some("display")
        );
        assert_eq!(
            json.get("capture_elapsed_ms").and_then(|v| v.as_u64()),
            Some(42)
        );
        assert_eq!(
            json.get("fallback_reason").and_then(|v| v.as_str()),
            Some("foreground_window_not_implemented")
        );
    }
}

use serde::{Deserialize, Serialize};
use voicewin_core::enhancement::PromptTemplate;
use voicewin_core::llm::VisualContextRuntime;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PromptPreviewContextOverride {
    #[serde(default)]
    pub clipboard: Option<String>,
    #[serde(default)]
    pub selected_text: Option<String>,
    #[serde(default)]
    pub window_context: Option<String>,
    #[serde(default)]
    pub screenshot_data_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSessionRequest {
    // Reserved for future use (e.g., manual transcript override / debug).
    pub transcript: String,

    // Optional non-fatal warning to attach to the session result (and persist to History).
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordingStatus {
    pub is_recording: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToggleRecordingResponse {
    pub status: RecordingStatus,
    pub stage: String,
    pub final_text: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSessionResponse {
    pub stage: String,
    pub final_text: Option<String>,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptPreviewRequest {
    pub prompt: PromptTemplate,
    pub transcript: String,
    #[serde(default)]
    pub forced_profile_id: Option<voicewin_core::types::ProfileId>,
    #[serde(default)]
    pub force_defaults: bool,
    #[serde(default)]
    pub context_override: Option<PromptPreviewContextOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptPreviewResponse {
    pub elapsed_ms: u64,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(default)]
    pub visual_context_runtime: Option<VisualContextRuntime>,
    pub app_process_name: Option<String>,
    pub app_window_title: Option<String>,
    pub matched_profile_name: Option<String>,
    pub provider_kind: String,
    pub api_kind: String,
    pub model: String,
    pub system_message: String,
    pub user_message: String,
    pub raw_output: String,
    pub final_output: String,
    #[serde(default)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeKind {
    Smoke,
    ScreenshotProductName,
}

impl Default for ProviderProbeKind {
    fn default() -> Self {
        Self::Smoke
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProbeRequest {
    pub provider_kind: String,
    pub api_kind: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub probe_kind: ProviderProbeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProbeResponse {
    pub probe_kind: ProviderProbeKind,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
    pub provider_kind: String,
    pub api_kind: String,
    pub model: String,
    pub expected_output: String,
    pub final_output: String,
    #[serde(default)]
    pub warning: Option<String>,
}

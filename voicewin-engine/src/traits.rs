use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use voicewin_core::types::AppIdentity;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioInput {
    // Audio is mono PCM samples at `sample_rate_hz`.
    // The engine expects that capture/resampling happened at the boundary.
    pub sample_rate_hz: u32,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnhancedText {
    pub text: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub clipboard: Option<String>,
    pub selected_text: Option<String>,
    pub window_context: Option<String>,
    pub custom_vocabulary: Option<String>,
}

#[async_trait]
pub trait AppContextProvider: Send + Sync {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity>;
    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot>;
}

#[async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(
        &self,
        audio: &AudioInput,
        provider: &str,
        model: &str,
        language: &str,
    ) -> anyhow::Result<Transcript>;
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn enhance(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        system_message: &str,
        user_message: &str,
    ) -> anyhow::Result<EnhancedText>;
}

#[async_trait]
pub trait Inserter: Send + Sync {
    async fn insert(
        &self,
        text: &str,
        mode: voicewin_core::types::InsertMode,
    ) -> anyhow::Result<()>;
}

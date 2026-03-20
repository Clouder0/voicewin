use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use voicewin_core::context::{ImageArtifact, ScreenshotCaptureMetadata, VisualCaptureScope};
use voicewin_core::types::AppIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenshotCaptureOptions {
    pub max_edge_px: u32,
    pub scope: VisualCaptureScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedScreenshot {
    pub image: ImageArtifact,
    #[serde(default)]
    pub metadata: ScreenshotCaptureMetadata,
}

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
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedScreenOcr {
    pub text: String,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub first_token_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub clipboard: Option<String>,
    pub selected_text: Option<String>,
    pub window_context: Option<String>,
    pub custom_vocabulary: Option<String>,
    pub screenshot: Option<ImageArtifact>,
    #[serde(default)]
    pub screenshot_metadata: Option<ScreenshotCaptureMetadata>,
    #[serde(default)]
    pub precomputed_screen_ocr: Option<PreparedScreenOcr>,
}

#[async_trait]
pub trait AppContextProvider: Send + Sync {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity>;
    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot>;

    async fn capture_screenshot(
        &self,
        _options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        Ok(None)
    }

    async fn snapshot_context_for_policy(
        &self,
        screenshot_options: Option<ScreenshotCaptureOptions>,
    ) -> anyhow::Result<ContextSnapshot> {
        let mut snapshot = self.snapshot_context().await?;
        if let Some(options) = screenshot_options.filter(|_| snapshot.screenshot.is_none()) {
            match self.capture_screenshot(options).await {
                Ok(Some(screenshot)) => {
                    log::debug!(
                        "snapshot_context_for_policy captured screenshot: requested_scope={:?} actual_scope={:?} elapsed_ms={:?} fallback_reason={:?}",
                        options.scope,
                        screenshot.metadata.actual_scope,
                        screenshot.metadata.capture_elapsed_ms,
                        screenshot.metadata.fallback_reason
                    );
                    snapshot.screenshot = Some(screenshot.image);
                    snapshot.screenshot_metadata = Some(screenshot.metadata);
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("capture_screenshot failed; continuing without screenshot: {e}");
                }
            }
        }
        Ok(snapshot)
    }
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
        provider_kind: &str,
        api_kind: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        system_message: &str,
        user_message: &str,
        attached_image: Option<&ImageArtifact>,
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

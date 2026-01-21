use crate::traits::{ContextSnapshot, EnhancedText, Transcript};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use voicewin_core::power_mode::EffectiveConfig;
use voicewin_core::types::{AppIdentity, InsertMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStage {
    Recording,
    Transcribing,
    Enhancing,
    Inserting,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTimings {
    pub transcription_ms: Option<u64>,
    pub enhancement_ms: Option<u64>,
}

impl Default for SessionTimings {
    fn default() -> Self {
        Self {
            transcription_ms: None,
            enhancement_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResult {
    pub stage: SessionStage,

    // A stable string label for UI display.
    // This is intentionally not derived from `Debug`.
    pub stage_label: Option<String>,

    pub app: AppIdentity,
    pub config: EffectiveConfig,
    pub transcript: Option<Transcript>,
    pub enhanced: Option<EnhancedText>,
    pub final_text: Option<String>,
    pub insert_mode: InsertMode,
    pub context: ContextSnapshot,
    pub timings: SessionTimings,
    pub error: Option<String>,
}

impl SessionResult {
    pub fn success(
        app: AppIdentity,
        config: EffectiveConfig,
        final_text: String,
        mode: InsertMode,
        ctx: ContextSnapshot,
    ) -> Self {
        Self {
            stage: SessionStage::Done,
            stage_label: Some("done".into()),
            app,
            config,
            transcript: None,
            enhanced: None,
            final_text: Some(final_text),
            insert_mode: mode,
            context: ctx,
            timings: SessionTimings::default(),
            error: None,
        }
    }

    pub fn failed(
        app: AppIdentity,
        config: EffectiveConfig,
        ctx: ContextSnapshot,
        error: impl Into<String>,
    ) -> Self {
        let insert_mode = config.insert_mode;
        Self {
            stage: SessionStage::Failed,
            stage_label: Some("failed".into()),
            app,
            config,
            transcript: None,
            enhanced: None,
            final_text: None,
            insert_mode,
            context: ctx,
            timings: SessionTimings::default(),
            error: Some(error.into()),
        }
    }
}

pub fn ms(d: Duration) -> u64 {
    d.as_millis().try_into().unwrap_or(u64::MAX)
}

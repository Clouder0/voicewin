use crate::session::{SessionResult, SessionStage, ms};
use crate::traits::{AppContextProvider, AudioInput, Inserter, LlmProvider, SttProvider};
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use voicewin_core::enhancement::{
    EnhancementContext, PromptTemplate, build_enhancement_prompt, detect_trigger_word,
    post_process_llm_output,
};
use voicewin_core::power_mode::{
    EphemeralOverrides, GlobalDefaults, PowerModeProfile, resolve_effective_config,
};
use voicewin_core::text::filter_transcription_output;
use voicewin_core::types::InsertMode;

const STAGE_RECORDING: &str = "recording";
const STAGE_TRANSCRIBING: &str = "transcribing";
const STAGE_ENHANCING: &str = "enhancing";
const STAGE_INSERTING: &str = "inserting";
const STAGE_DONE: &str = "done";

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("no default prompt configured")]
    NoDefaultPrompt,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub defaults: GlobalDefaults,
    pub profiles: Vec<PowerModeProfile>,
    pub prompts: Vec<PromptTemplate>,

    // LLM auth is currently global in MVP.
    pub llm_api_key: String,
}

pub struct VoicewinEngine {
    cfg: EngineConfig,
    context_provider: Arc<dyn AppContextProvider>,
    stt: Arc<dyn SttProvider>,
    llm: Arc<dyn LlmProvider>,
    inserter: Arc<dyn Inserter>,
}

impl VoicewinEngine {
    pub fn new(
        cfg: EngineConfig,
        context_provider: Arc<dyn AppContextProvider>,
        stt: Arc<dyn SttProvider>,
        llm: Arc<dyn LlmProvider>,
        inserter: Arc<dyn Inserter>,
    ) -> Self {
        Self {
            cfg,
            context_provider,
            stt,
            llm,
            inserter,
        }
    }

    /// Runs the full pipeline (transcribe -> optional enhance -> insert).
    pub async fn run_session(&self, audio: AudioInput) -> anyhow::Result<SessionResult> {
        self.run_session_with_hook(audio, |_stage| async {}).await
    }

    /// Same as `run_session`, but emits a stage hook as the pipeline progresses.
    ///
    /// The hook is intended for UI progress (e.g. overlay HUD) and must be fast.
    pub async fn run_session_with_hook<F, Fut>(
        &self,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<SessionResult>
    where
        F: Fn(&'static str) -> Fut,
        Fut: Future<Output = ()>,
    {
        let app = self.context_provider.foreground_app().await?;
        let ctx_snapshot = self
            .context_provider
            .snapshot_context()
            .await
            .unwrap_or_default();

        let ephemeral = EphemeralOverrides::default();
        let eff =
            resolve_effective_config(&self.cfg.defaults, &self.cfg.profiles, &app, &ephemeral);

        // Build a result shell; we will fill `final_text` before insertion so it is recoverable.
        let mut result = SessionResult::success(
            app.clone(),
            eff.clone(),
            String::new(),
            eff.insert_mode,
            ctx_snapshot.clone(),
        );

        // 0) Recording (performed by caller)
        result.stage = SessionStage::Recording;
        result.stage_label = Some(STAGE_RECORDING.into());
        on_stage(STAGE_RECORDING).await;

        // 1) Transcribe
        result.stage = SessionStage::Transcribing;
        result.stage_label = Some(STAGE_TRANSCRIBING.into());
        on_stage(STAGE_TRANSCRIBING).await;

        let t0 = Instant::now();
        let transcript = self
            .stt
            .transcribe(&audio, &eff.stt_provider, &eff.stt_model, &eff.language)
            .await?;
        let transcription_ms = ms(t0.elapsed());

        let mut final_text = filter_transcription_output(&transcript.text);

        // 2) Trigger word prompt override (VoiceInk behavior)
        let mut prompt_id = eff.prompt_id.clone();
        let detection = detect_trigger_word(&final_text, &self.cfg.prompts);
        if detection.should_enable_enhancement {
            final_text = detection.processed_transcript;
            prompt_id = detection.selected_prompt_id;
        }

        let mut enhanced = None;
        let mut enhancement_ms = None;
        if eff.enable_enhancement || detection.should_enable_enhancement {
            // 3) Enhance
            result.stage = SessionStage::Enhancing;
            result.stage_label = Some(STAGE_ENHANCING.into());
            on_stage(STAGE_ENHANCING).await;

            let selected = prompt_id
                .as_ref()
                .and_then(|id| self.cfg.prompts.iter().find(|p| &p.id == id))
                .or_else(|| self.cfg.prompts.first());

            let prompt = selected.ok_or(EngineError::NoDefaultPrompt)?;

            let ctx = EnhancementContext {
                clipboard_context: eff
                    .context
                    .use_clipboard
                    .then(|| ctx_snapshot.clipboard.clone())
                    .flatten(),
                currently_selected_text: eff
                    .context
                    .use_selected_text
                    .then(|| ctx_snapshot.selected_text.clone())
                    .flatten(),
                current_window_context: eff
                    .context
                    .use_window_context
                    .then(|| ctx_snapshot.window_context.clone())
                    .flatten(),
                custom_vocabulary: eff
                    .context
                    .use_custom_vocabulary
                    .then(|| ctx_snapshot.custom_vocabulary.clone())
                    .flatten(),
            };

            let built = build_enhancement_prompt(&final_text, prompt, &ctx);

            let e0 = Instant::now();
            let llm_out = self
                .llm
                .enhance(
                    &eff.llm_base_url,
                    &self.cfg.llm_api_key,
                    &eff.llm_model,
                    &built.system_message,
                    &built.user_message,
                )
                .await?;
            enhancement_ms = Some(ms(e0.elapsed()));

            let cleaned = post_process_llm_output(&llm_out.text);
            final_text = cleaned;
            enhanced = Some(llm_out);
        }

        // Make the output text recoverable even if insertion fails.
        result.final_text = Some(final_text.clone());

        // 4) Insert
        result.stage = SessionStage::Inserting;
        result.stage_label = Some(STAGE_INSERTING.into());
        on_stage(STAGE_INSERTING).await;

        let mode: InsertMode = eff.insert_mode;
        if let Err(e) = self.inserter.insert(&final_text, mode).await {
            result.stage = SessionStage::Failed;
            result.stage_label = Some("failed".into());
            result.transcript = Some(transcript);
            result.enhanced = enhanced;
            result.timings.transcription_ms = Some(transcription_ms);
            result.timings.enhancement_ms = enhancement_ms;
            result.error = Some(e.to_string());
            return Ok(result);
        }

        result.stage = SessionStage::Done;
        result.stage_label = Some(STAGE_DONE.into());
        result.transcript = Some(transcript);
        result.enhanced = enhanced;
        result.timings.transcription_ms = Some(transcription_ms);
        result.timings.enhancement_ms = enhancement_ms;
        Ok(result)
    }
}

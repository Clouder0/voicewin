use crate::session::{SessionResult, SessionStage, ms};
use crate::traits::{
    AppContextProvider, AudioInput, Inserter, LlmProvider, PreparedScreenOcr,
    ScreenshotCaptureOptions, SttProvider,
};
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use voicewin_core::enhancement::{
    EnhancementContext, PromptTemplate, build_enhancement_prompt, detect_trigger_word,
    post_process_llm_output_with_screen_ocr,
};
use voicewin_core::llm::{
    ScreenOcrSource, VisualContextDispatch, VisualContextRuntime, ocr_sidecar_api_kind,
    resolve_visual_context_dispatch, screenshot_context_warning, visual_capture_scope_label,
    visual_context_capture_unavailable_warning,
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
const SCREEN_OCR_SYSTEM_MESSAGE: &str = "Extract visible text from the attached screenshot. Return only the recognized text as plain text. Preserve line breaks, casing, punctuation, and spelling as accurately as possible. Do not explain, summarize, or describe the screenshot. If no readable text is present, return an empty response.";
const SCREEN_OCR_USER_MESSAGE: &str =
    "<OCR_TASK>\nReturn only the text recognized from the attached screenshot.\n</OCR_TASK>";

#[derive(Debug, Clone)]
struct ScreenOcrResult {
    text: String,
    elapsed_ms: u64,
    first_token_ms: Option<u64>,
}

fn screen_ocr_result_from_precomputed(ocr: &PreparedScreenOcr) -> Option<ScreenOcrResult> {
    let text = ocr.text.trim().to_string();
    (!text.is_empty()).then_some(ScreenOcrResult {
        text,
        elapsed_ms: ocr.elapsed_ms,
        first_token_ms: ocr.first_token_ms,
    })
}

fn screenshot_capture_options_for_effective(
    eff: &voicewin_core::power_mode::EffectiveConfig,
) -> Option<ScreenshotCaptureOptions> {
    (!matches!(
        resolve_visual_context_dispatch(
            eff.context.visual_context_mode,
            &eff.llm_provider_kind,
            &eff.llm_api_kind
        ),
        VisualContextDispatch::Off
    ))
    .then_some(ScreenshotCaptureOptions {
        max_edge_px: eff.screenshot_max_edge_px,
        scope: eff.context.visual_capture_scope,
    })
}

fn apply_screenshot_capture_metadata(
    runtime: &mut VisualContextRuntime,
    snapshot: &crate::traits::ContextSnapshot,
) {
    if let Some(metadata) = snapshot.screenshot_metadata.as_ref() {
        runtime.capture_actual_scope = metadata.actual_scope;
        runtime.screenshot_capture_elapsed_ms = metadata.capture_elapsed_ms;
        runtime.capture_fallback_reason = metadata.fallback_reason.clone();
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("no default prompt configured")]
    NoDefaultPrompt,
}

#[derive(Clone)]
pub struct EngineConfig {
    pub defaults: GlobalDefaults,
    pub profiles: Vec<PowerModeProfile>,
    pub prompts: Vec<PromptTemplate>,

    // Provider auth is global in MVP, but profiles can select which one to use.
    pub openai_api_key: String,
    pub gemini_api_key: String,
}

impl std::fmt::Debug for EngineConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineConfig")
            .field("defaults", &self.defaults)
            .field("profiles", &self.profiles)
            .field("prompts", &self.prompts)
            .field("openai_api_key", &"[REDACTED]")
            .field("gemini_api_key", &"[REDACTED]")
            .finish()
    }
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
        let ephemeral = EphemeralOverrides::default();
        let eff =
            resolve_effective_config(&self.cfg.defaults, &self.cfg.profiles, &app, &ephemeral);
        let ctx_snapshot = self
            .context_provider
            .snapshot_context_for_policy(screenshot_capture_options_for_effective(&eff))
            .await
            .unwrap_or_default();

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

        self.run_post_stt_pipeline(
            result,
            eff,
            ctx_snapshot,
            transcript,
            Some(transcription_ms),
            on_stage,
        )
        .await
    }

    /// Runs the post-STT pipeline (optional enhance -> insert) given a transcript.
    ///
    /// Used by realtime providers to reuse the same enhancement/insertion logic.
    pub async fn run_session_with_transcript_with_hook<F, Fut>(
        &self,
        transcript_text: String,
        on_stage: F,
    ) -> anyhow::Result<SessionResult>
    where
        F: Fn(&'static str) -> Fut,
        Fut: Future<Output = ()>,
    {
        let app = self.context_provider.foreground_app().await?;
        let ephemeral = EphemeralOverrides::default();
        let eff =
            resolve_effective_config(&self.cfg.defaults, &self.cfg.profiles, &app, &ephemeral);
        let ctx_snapshot = self
            .context_provider
            .snapshot_context_for_policy(screenshot_capture_options_for_effective(&eff))
            .await
            .unwrap_or_default();

        let mut result = SessionResult::success(
            app.clone(),
            eff.clone(),
            String::new(),
            eff.insert_mode,
            ctx_snapshot.clone(),
        );

        result.stage = SessionStage::Recording;
        result.stage_label = Some(STAGE_RECORDING.into());
        on_stage(STAGE_RECORDING).await;

        result.stage = SessionStage::Transcribing;
        result.stage_label = Some(STAGE_TRANSCRIBING.into());
        on_stage(STAGE_TRANSCRIBING).await;

        let transcript = crate::traits::Transcript {
            text: transcript_text,
            provider: eff.stt_provider.clone(),
            model: eff.stt_model.clone(),
        };

        self.run_post_stt_pipeline(result, eff, ctx_snapshot, transcript, None, on_stage)
            .await
    }

    async fn run_post_stt_pipeline<F, Fut>(
        &self,
        mut result: SessionResult,
        eff: voicewin_core::power_mode::EffectiveConfig,
        ctx_snapshot: crate::traits::ContextSnapshot,
        transcript: crate::traits::Transcript,
        transcription_ms: Option<u64>,
        on_stage: F,
    ) -> anyhow::Result<SessionResult>
    where
        F: Fn(&'static str) -> Fut,
        Fut: Future<Output = ()>,
    {
        let mut final_text = filter_transcription_output(&transcript.text);
        result.visual_context.mode = eff.context.visual_context_mode;
        result.visual_context.capture_scope = eff.context.visual_capture_scope;
        apply_screenshot_capture_metadata(&mut result.visual_context, &ctx_snapshot);

        if final_text.trim().is_empty() {
            result.stage = SessionStage::Failed;
            result.stage_label = Some("failed".into());
            result.transcript = Some(transcript);
            result.timings.transcription_ms = transcription_ms;
            result.error = Some(
                "No speech detected. Try speaking louder or selecting the correct microphone."
                    .into(),
            );
            return Ok(result);
        }

        let llm_api_key = match eff.llm_provider_kind.trim() {
            "gemini" => self.cfg.gemini_api_key.as_str(),
            _ => self.cfg.openai_api_key.as_str(),
        };
        let has_llm_key = !llm_api_key.trim().is_empty();

        // Trigger word prompt override (VoiceInk behavior)
        let mut prompt_id = eff.prompt_id.clone();
        let detection = detect_trigger_word(&final_text, &self.cfg.prompts);
        let trigger_word_applied = has_llm_key && detection.should_enable_enhancement;
        if trigger_word_applied {
            final_text = detection.processed_transcript;
            prompt_id = detection.selected_prompt_id;
            result.detected_trigger_word = detection.detected_trigger_word.clone();
        }

        let mut enhanced = None;
        let mut enhancement_ms = None;
        let mut enhancement_first_token_ms = None;

        let wants_enhancement = eff.enable_enhancement || detection.should_enable_enhancement;
        if wants_enhancement && has_llm_key {
            result.stage = SessionStage::Enhancing;
            result.stage_label = Some(STAGE_ENHANCING.into());
            on_stage(STAGE_ENHANCING).await;

            let selected = prompt_id
                .as_ref()
                .and_then(|id| self.cfg.prompts.iter().find(|p| &p.id == id))
                .or_else(|| self.cfg.prompts.first());

            let prompt = selected.ok_or(EngineError::NoDefaultPrompt)?;
            result.prompt_id = Some(prompt.id.0.to_string());
            result.prompt_title = Some(prompt.title.clone());
            let requested_visual_dispatch = resolve_visual_context_dispatch(
                eff.context.visual_context_mode,
                &eff.llm_provider_kind,
                &eff.llm_api_kind,
            );
            log::debug!(
                "visual context dispatch: mode={:?} requested_dispatch={:?} provider_kind={} api_kind={} capture_scope={} screenshot_present={}",
                eff.context.visual_context_mode,
                requested_visual_dispatch,
                eff.llm_provider_kind,
                eff.llm_api_kind,
                visual_capture_scope_label(eff.context.visual_capture_scope),
                ctx_snapshot.screenshot.is_some()
            );
            if matches!(
                eff.context.visual_context_mode,
                voicewin_core::context::VisualContextMode::Screenshot
            ) && matches!(requested_visual_dispatch, VisualContextDispatch::Off)
            {
                if let Some(warning) =
                    screenshot_context_warning(&eff.llm_provider_kind, &eff.llm_api_kind)
                {
                    result.push_warning(warning);
                }
            }
            let (screen_ocr_result, screen_ocr_source) = if matches!(
                requested_visual_dispatch,
                VisualContextDispatch::Ocr
            ) {
                if let Some(precomputed) = ctx_snapshot
                    .precomputed_screen_ocr
                    .as_ref()
                    .and_then(screen_ocr_result_from_precomputed)
                {
                    log::debug!(
                        "visual context using precomputed OCR from prepared context: elapsed_ms={} chars={} first_token_ms={:?}",
                        precomputed.elapsed_ms,
                        precomputed.text.chars().count(),
                        precomputed.first_token_ms
                    );
                    (Some(precomputed), Some(ScreenOcrSource::Prepared))
                } else {
                    match ctx_snapshot.screenshot.as_ref() {
                        Some(screenshot) => match self
                            .extract_screen_ocr_text(&eff, llm_api_key, screenshot)
                            .await
                        {
                            Ok(ocr) => (Some(ocr), Some(ScreenOcrSource::Inline)),
                            Err(e) => {
                                let mut msg = e.to_string();
                                if msg.len() > 140 {
                                    msg.truncate(140);
                                    msg.push_str("...");
                                }
                                result.push_warning(format!(
                                    "Visual OCR failed; continuing without OCR text. ({msg})"
                                ));
                                (None, None)
                            }
                        },
                        None => {
                            log::debug!(
                                "visual context ocr requested but no screenshot artifact or precomputed OCR was available"
                            );
                            (None, None)
                        }
                    }
                }
            } else {
                (None, None)
            };
            if let Some(ocr) = screen_ocr_result.as_ref() {
                result.visual_context.screen_ocr_source = screen_ocr_source;
                result.visual_context.screen_ocr_elapsed_ms = Some(ocr.elapsed_ms);
                result.visual_context.screen_ocr_first_token_ms = ocr.first_token_ms;
                result.visual_context.screen_ocr_text_chars =
                    Some(ocr.text.chars().count().try_into().unwrap_or(u64::MAX));
            }
            let screen_ocr_text = match screen_ocr_result.as_ref() {
                Some(ocr) if !ocr.text.trim().is_empty() => Some(ocr.text.clone()),
                Some(_) => {
                    log::debug!(
                        "visual context ocr produced empty text; continuing without OCR context"
                    );
                    None
                }
                None => None,
            };
            let attached_screenshot =
                if matches!(requested_visual_dispatch, VisualContextDispatch::Screenshot) {
                    ctx_snapshot.screenshot.clone()
                } else {
                    None
                };
            if matches!(requested_visual_dispatch, VisualContextDispatch::Screenshot)
                && attached_screenshot.is_none()
            {
                if let Some(warning) = visual_context_capture_unavailable_warning(
                    requested_visual_dispatch,
                    eff.context.visual_capture_scope,
                ) {
                    result.push_warning(warning);
                }
            } else if matches!(requested_visual_dispatch, VisualContextDispatch::Ocr)
                && screen_ocr_text.is_none()
                && ctx_snapshot.screenshot.is_none()
                && ctx_snapshot.precomputed_screen_ocr.is_none()
            {
                if let Some(warning) = visual_context_capture_unavailable_warning(
                    requested_visual_dispatch,
                    eff.context.visual_capture_scope,
                ) {
                    result.push_warning(warning);
                }
            }
            result.visual_context.dispatch = if attached_screenshot.is_some() {
                VisualContextDispatch::Screenshot
            } else if screen_ocr_text.is_some() {
                VisualContextDispatch::Ocr
            } else {
                VisualContextDispatch::Off
            };
            log::debug!(
                "visual context final: mode={:?} requested_dispatch={:?} actual_dispatch={:?} provider_kind={} api_kind={} capture_scope={} capture_actual_scope={:?} screenshot_capture_elapsed_ms={:?} capture_fallback_reason={:?} screenshot_attached={} screen_ocr_source={:?} screen_ocr_elapsed_ms={:?} screen_ocr_text_chars={:?}",
                eff.context.visual_context_mode,
                requested_visual_dispatch,
                result.visual_context.dispatch,
                eff.llm_provider_kind,
                eff.llm_api_kind,
                visual_capture_scope_label(eff.context.visual_capture_scope),
                result.visual_context.capture_actual_scope,
                result.visual_context.screenshot_capture_elapsed_ms,
                result.visual_context.capture_fallback_reason.as_deref(),
                attached_screenshot.is_some(),
                result.visual_context.screen_ocr_source,
                result.visual_context.screen_ocr_elapsed_ms,
                result.visual_context.screen_ocr_text_chars
            );

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
                screen_ocr_text,
                screenshot: attached_screenshot.clone(),
            };

            let built = build_enhancement_prompt(&final_text, prompt, &ctx);

            let e0 = Instant::now();
            match self
                .llm
                .enhance(
                    &eff.llm_provider_kind,
                    &eff.llm_api_kind,
                    &eff.llm_base_url,
                    llm_api_key,
                    &eff.llm_model,
                    eff.llm_reasoning_effort.as_deref(),
                    &built.system_message,
                    &built.user_message,
                    ctx.screenshot.as_ref(),
                )
                .await
            {
                Ok(llm_out) => {
                    enhancement_ms = Some(ms(e0.elapsed()));
                    enhancement_first_token_ms = llm_out.first_token_ms;
                    let cleaned = post_process_llm_output_with_screen_ocr(
                        &llm_out.text,
                        prompt.mode.clone(),
                        &final_text,
                        ctx.screen_ocr_text.as_deref(),
                    );
                    if let Some(warning) = cleaned.warning.as_deref() {
                        result.push_warning(warning);
                    }
                    final_text = cleaned.text;
                    enhanced = Some(llm_out);
                }
                Err(e) => {
                    let mut msg = e.to_string();
                    if msg.len() > 140 {
                        msg.truncate(140);
                        msg.push_str("...");
                    }
                    result.push_warning(format!(
                        "Enhancement failed; inserted raw transcript. ({msg})"
                    ));
                }
            }
        }

        result.final_text = Some(final_text.clone());

        result.stage = SessionStage::Inserting;
        result.stage_label = Some(STAGE_INSERTING.into());
        on_stage(STAGE_INSERTING).await;

        let mode: InsertMode = eff.insert_mode;
        if let Err(e) = self.inserter.insert(&final_text, mode).await {
            result.stage = SessionStage::Failed;
            result.stage_label = Some("failed".into());
            result.transcript = Some(transcript);
            result.enhanced = enhanced;
            result.timings.transcription_ms = transcription_ms;
            result.timings.enhancement_ms = enhancement_ms;
            result.timings.enhancement_first_token_ms = enhancement_first_token_ms;
            result.error = Some(e.to_string());
            return Ok(result);
        }

        result.stage = SessionStage::Done;
        result.stage_label = Some(STAGE_DONE.into());
        result.transcript = Some(transcript);
        result.enhanced = enhanced;
        result.timings.transcription_ms = transcription_ms;
        result.timings.enhancement_ms = enhancement_ms;
        result.timings.enhancement_first_token_ms = enhancement_first_token_ms;
        Ok(result)
    }

    async fn extract_screen_ocr_text(
        &self,
        eff: &voicewin_core::power_mode::EffectiveConfig,
        llm_api_key: &str,
        screenshot: &voicewin_core::context::ImageArtifact,
    ) -> anyhow::Result<ScreenOcrResult> {
        let Some(ocr_api_kind) = ocr_sidecar_api_kind(&eff.llm_provider_kind, &eff.llm_api_kind)
        else {
            anyhow::bail!(
                "no OCR-capable API mapping for provider kind: {}",
                eff.llm_provider_kind
            );
        };

        let started = Instant::now();
        log::debug!(
            "visual context ocr start: provider_kind={} selected_api_kind={} ocr_api_kind={} model={} capture_scope={} screenshot_bytes={}",
            eff.llm_provider_kind,
            eff.llm_api_kind,
            ocr_api_kind,
            eff.llm_model,
            visual_capture_scope_label(eff.context.visual_capture_scope),
            screenshot.data_url.len()
        );
        let response = match self
            .llm
            .enhance(
                &eff.llm_provider_kind,
                ocr_api_kind,
                &eff.llm_base_url,
                llm_api_key,
                &eff.llm_model,
                eff.llm_reasoning_effort.as_deref(),
                SCREEN_OCR_SYSTEM_MESSAGE,
                SCREEN_OCR_USER_MESSAGE,
                Some(screenshot),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                log::warn!(
                    "visual context ocr failed: provider_kind={} ocr_api_kind={} elapsed_ms={} error={}",
                    eff.llm_provider_kind,
                    ocr_api_kind,
                    started.elapsed().as_millis(),
                    error
                );
                return Err(error);
            }
        };
        let text = response.text.trim().to_string();
        let elapsed_ms = ms(started.elapsed());
        log::debug!(
            "visual context ocr done: provider_kind={} ocr_api_kind={} elapsed_ms={} chars={} first_token_ms={:?}",
            eff.llm_provider_kind,
            ocr_api_kind,
            elapsed_ms,
            text.len(),
            response.first_token_ms
        );
        Ok(ScreenOcrResult {
            text,
            elapsed_ms,
            first_token_ms: response.first_token_ms,
        })
    }
}

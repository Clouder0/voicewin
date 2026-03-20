use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{collections::HashSet, path::Path};

use anyhow::Context;
use voicewin_core::config::AppConfig;
use voicewin_core::enhancement::{
    EnhancementContext, PromptMode, PromptTemplate, build_enhancement_prompt,
    post_process_llm_output, post_process_llm_output_with_screen_ocr,
};
use voicewin_core::llm::{
    ScreenOcrSource, VisualContextDispatch, VisualContextRuntime, llm_supports_attached_images,
    ocr_sidecar_api_kind, resolve_visual_context_dispatch, screenshot_context_warning,
    visual_capture_scope_label, visual_context_capture_unavailable_warning,
};
use voicewin_core::power_mode::{EffectiveConfig, EphemeralOverrides, resolve_effective_config};
use voicewin_core::types::{AppIdentity, ProfileId, PromptId};
use voicewin_engine::engine::VoicewinEngine;
use voicewin_engine::traits::{
    AppContextProvider, AudioInput, CapturedScreenshot, ContextSnapshot, Inserter, LlmProvider,
    PreparedScreenOcr, ScreenshotCaptureOptions,
};

#[cfg(any(windows, target_os = "macos"))]
use voicewin_audio::{AudioCaptureError, AudioRecorder};
use voicewin_runtime::config_store::ConfigStore;
use voicewin_runtime::ipc::{
    PromptPreviewResponse, ProviderProbeKind, ProviderProbeResponse, RunSessionRequest,
    RunSessionResponse,
};

#[cfg(any(windows, target_os = "macos"))]
pub fn user_facing_audio_error(e: &voicewin_audio::AudioCaptureError) -> String {
    let raw = e.to_string();

    // Keep messages actionable and short; details are in logs.
    if raw.contains("NoInputDevice") || raw.to_lowercase().contains("no input device") {
        return "No microphone detected. Check your mic and choose the device in the app.".into();
    }

    if raw.to_lowercase().contains("permission") || raw.to_lowercase().contains("access") {
        #[cfg(windows)]
        {
            return "Microphone access appears blocked. Check Windows Settings > Privacy & security > Microphone.".into();
        }

        #[cfg(target_os = "macos")]
        {
            return "Microphone access appears blocked. Check System Settings > Privacy & Security > Microphone.".into();
        }
    }

    "Audio recording failed. See History for recovery and check logs for details.".into()
}

use voicewin_runtime::runtime_engine::build_engine_from_config;
use voicewin_runtime::secrets::{
    SecretKey, configure_secret_store_path, delete_secret_at_path, get_secret_at_path,
    set_secret_at_path,
};

fn elapsed_ms_floor_one(started: Instant) -> u64 {
    started
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
        .max(1)
}

const PROVIDER_PROBE_SCREENSHOT_DATA_URL: &str =
    include_str!("../assets/provider_probe_voicewin_640.data-url.txt");

const PROVIDER_PROBE_SMOKE_OUTPUT: &str = "VoiceWin provider probe ok.";
const PROVIDER_PROBE_SCREENSHOT_OUTPUT: &str = "VoiceWin";
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
    effective: &EffectiveConfig,
) -> Option<ScreenshotCaptureOptions> {
    (!matches!(
        resolve_visual_context_dispatch(
            effective.context.visual_context_mode,
            &effective.llm_provider_kind,
            &effective.llm_api_kind
        ),
        VisualContextDispatch::Off
    ))
    .then_some(ScreenshotCaptureOptions {
        max_edge_px: effective.screenshot_max_edge_px,
        scope: effective.context.visual_capture_scope,
    })
}

fn screenshot_context_warning_for_effective(effective: &EffectiveConfig) -> Option<String> {
    matches!(
        effective.context.visual_context_mode,
        voicewin_core::context::VisualContextMode::Screenshot
    )
    .then(|| screenshot_context_warning(&effective.llm_provider_kind, &effective.llm_api_kind))
    .flatten()
}

fn apply_screenshot_capture_metadata(
    runtime: &mut VisualContextRuntime,
    snapshot: &ContextSnapshot,
) {
    if let Some(metadata) = snapshot.screenshot_metadata.as_ref() {
        runtime.capture_actual_scope = metadata.actual_scope;
        runtime.screenshot_capture_elapsed_ms = metadata.capture_elapsed_ms;
        runtime.capture_fallback_reason = metadata.fallback_reason.clone();
    }
}

fn merge_message(dst: &mut Option<String>, msg: impl AsRef<str>) {
    let msg = msg.as_ref().trim();
    if msg.is_empty() {
        return;
    }

    *dst = match dst.take() {
        Some(existing) if !existing.trim().is_empty() => Some(format!("{existing} | {msg}")),
        _ => Some(msg.to_string()),
    };
}

fn normalize_microphone_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

pub fn microphone_selection_changed(
    previous_id: Option<&str>,
    previous_name: Option<&str>,
    next_id: Option<&str>,
    next_name: Option<&str>,
) -> bool {
    normalize_microphone_value(previous_id) != normalize_microphone_value(next_id)
        || normalize_microphone_value(previous_name) != normalize_microphone_value(next_name)
}

pub fn microphone_device_changed(previous: Option<&str>, next: Option<&str>) -> bool {
    microphone_selection_changed(None, previous, None, next)
}

pub struct PreparedSessionContext {
    cfg: AppConfig,
    pub app: AppIdentity,
    pub context: ContextSnapshot,
    pub effective_config: EffectiveConfig,
    preflight_abort: Option<tokio::task::AbortHandle>,
}

impl std::fmt::Debug for PreparedSessionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSessionContext")
            .field("cfg", &self.cfg)
            .field("app", &self.app)
            .field("context", &self.context)
            .field("effective_config", &self.effective_config)
            .field(
                "has_preflight_abort",
                &self.preflight_abort.as_ref().map(|_| true).unwrap_or(false),
            )
            .finish()
    }
}

impl PreparedSessionContext {
    fn abort_preflight(&self) {
        if let Some(handle) = self.preflight_abort.as_ref() {
            handle.abort();
        }
    }
}

struct PreflightAbortGuard {
    abort_handle: Option<tokio::task::AbortHandle>,
}

impl PreflightAbortGuard {
    fn new(abort_handle: tokio::task::AbortHandle) -> Self {
        Self {
            abort_handle: Some(abort_handle),
        }
    }

    fn into_abort_handle(mut self) -> tokio::task::AbortHandle {
        self.abort_handle
            .take()
            .expect("preflight abort handle should be present")
    }
}

impl Drop for PreflightAbortGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.abort_handle.as_ref() {
            handle.abort();
        }
    }
}

#[derive(Debug, Clone)]
struct FixedAppContextProvider {
    app: AppIdentity,
    context: ContextSnapshot,
}

impl FixedAppContextProvider {
    fn new(app: AppIdentity, context: ContextSnapshot) -> Self {
        Self { app, context }
    }

    fn boxed(self) -> Arc<dyn AppContextProvider> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl AppContextProvider for FixedAppContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(self.app.clone())
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(self.context.clone())
    }

    async fn capture_screenshot(
        &self,
        _options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        Ok(self
            .context
            .screenshot
            .clone()
            .map(|image| CapturedScreenshot {
                image,
                metadata: self.context.screenshot_metadata.clone().unwrap_or_default(),
            }))
    }
}

#[derive(Clone)]
struct VocabularyOverlayContextProvider {
    inner: Arc<dyn AppContextProvider>,
    custom_vocabulary: Option<String>,
}

impl VocabularyOverlayContextProvider {
    fn new(inner: Arc<dyn AppContextProvider>, custom_vocabulary: Option<String>) -> Self {
        Self {
            inner,
            custom_vocabulary,
        }
    }

    fn boxed(self) -> Arc<dyn AppContextProvider> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl AppContextProvider for VocabularyOverlayContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        self.inner.foreground_app().await
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        let snapshot = self.inner.snapshot_context().await?;
        Ok(merge_custom_vocabulary(
            snapshot,
            self.custom_vocabulary.clone(),
        ))
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        self.inner.capture_screenshot(options).await
    }
}

#[derive(Clone)]
pub struct AppService {
    config_store: ConfigStore,
    secrets_path: PathBuf,
    ctx: Arc<dyn AppContextProvider>,
    inserter: Arc<dyn Inserter>,
    llm_provider: Arc<voicewin_runtime::llm::OpenAiCompatibleLlmProvider>,

    #[cfg(any(windows, target_os = "macos"))]
    recorder: Arc<tokio::sync::Mutex<Option<AudioRecorder>>>,
}

impl AppService {
    pub fn new(
        config_path: PathBuf,
        ctx: Arc<dyn AppContextProvider>,
        inserter: Arc<dyn Inserter>,
    ) -> Self {
        let secrets_path = config_path
            .parent()
            .map(|p| p.join("secrets.json"))
            .unwrap_or_else(|| PathBuf::from("secrets.json"));
        configure_secret_store_path(&secrets_path);

        Self {
            config_store: ConfigStore::at_path(config_path),
            secrets_path,
            ctx,
            inserter,
            llm_provider: Arc::new(
                voicewin_runtime::llm::OpenAiCompatibleLlmProvider::new()
                    .expect("build llm http client"),
            ),
            #[cfg(any(windows, target_os = "macos"))]
            recorder: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn start_recording(&self) -> Result<(), AudioCaptureError> {
        let mut recorder = self.recorder.lock().await;
        if recorder.is_none() {
            let cfg = self.load_config().ok();
            let preferred_id = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device_id.as_deref());
            let preferred_name = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device.as_deref());
            *recorder = Some(AudioRecorder::open_preferred(preferred_id, preferred_name)?);
        }
        recorder
            .as_ref()
            .ok_or(AudioCaptureError::NoInputDevice)?
            .start()
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn stop_recording(&self) -> Result<AudioInput, AudioCaptureError> {
        let mut recorder = self.recorder.lock().await;
        let r = recorder.as_mut().ok_or(AudioCaptureError::NoInputDevice)?;

        let captured = r.stop_captured()?;

        let samples = if captured.sample_rate_hz == 16_000 {
            captured.samples
        } else {
            AudioRecorder::resample_to_16k(&captured.samples, captured.sample_rate_hz)?
        };

        Ok(AudioInput {
            sample_rate_hz: 16_000,
            samples,
        })
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn cancel_recording(&self) -> Result<(), AudioCaptureError> {
        // Best-effort: stop and discard captured audio.
        let mut recorder = self.recorder.lock().await;
        let Some(r) = recorder.as_mut() else {
            return Ok(());
        };

        let _ = r.stop();
        Ok(())
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn invalidate_recorder(&self) {
        let mut recorder = self.recorder.lock().await;
        if let Some(r) = recorder.take() {
            let _ = r.close();
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn start_recording_with_level_callback<F>(
        &self,
        cb: F,
    ) -> Result<(), AudioCaptureError>
    where
        F: Fn(&[f32]) + Send + Sync + 'static,
    {
        // Set callback first, then start.
        let mut recorder = self.recorder.lock().await;
        if recorder.is_none() {
            let cfg = self.load_config().ok();
            let preferred_id = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device_id.as_deref());
            let preferred_name = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device.as_deref());
            *recorder = Some(AudioRecorder::open_preferred(preferred_id, preferred_name)?);
        }
        let r = recorder.as_ref().ok_or(AudioCaptureError::NoInputDevice)?;

        r.set_level_callback(cb);
        r.start()
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn recording_sample_rate_hz(&self) -> Option<u32> {
        let recorder = self.recorder.lock().await;
        recorder.as_ref().map(|r| r.sample_rate_hz())
    }

    pub fn load_config(&self) -> anyhow::Result<AppConfig> {
        self.config_store.load()
    }

    pub fn save_config(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        self.config_store.save(cfg)
    }

    pub fn set_openai_api_key(&self, value: &str) -> anyhow::Result<()> {
        set_secret_at_path(&self.secrets_path, SecretKey::OpenAiCompatibleApiKey, value)
    }

    pub fn get_openai_api_key_present(&self) -> anyhow::Result<bool> {
        Ok(get_secret_at_path(&self.secrets_path, SecretKey::OpenAiCompatibleApiKey)?.is_some())
    }

    pub fn clear_openai_api_key(&self) -> anyhow::Result<()> {
        delete_secret_at_path(&self.secrets_path, SecretKey::OpenAiCompatibleApiKey)
    }

    pub fn set_gemini_api_key(&self, value: &str) -> anyhow::Result<()> {
        set_secret_at_path(&self.secrets_path, SecretKey::GeminiApiKey, value)
    }

    pub fn get_gemini_api_key_present(&self) -> anyhow::Result<bool> {
        Ok(get_secret_at_path(&self.secrets_path, SecretKey::GeminiApiKey)?.is_some())
    }

    pub fn clear_gemini_api_key(&self) -> anyhow::Result<()> {
        delete_secret_at_path(&self.secrets_path, SecretKey::GeminiApiKey)
    }

    pub fn set_elevenlabs_api_key(&self, value: &str) -> anyhow::Result<()> {
        set_secret_at_path(&self.secrets_path, SecretKey::ElevenLabsApiKey, value)
    }

    pub fn clear_elevenlabs_api_key(&self) -> anyhow::Result<()> {
        delete_secret_at_path(&self.secrets_path, SecretKey::ElevenLabsApiKey)
    }

    pub fn get_elevenlabs_api_key_present(&self) -> anyhow::Result<bool> {
        Ok(get_secret_at_path(&self.secrets_path, SecretKey::ElevenLabsApiKey)?.is_some())
    }

    pub fn clear_api_keys(&self) -> anyhow::Result<()> {
        delete_secret_at_path(&self.secrets_path, SecretKey::OpenAiCompatibleApiKey)?;
        delete_secret_at_path(&self.secrets_path, SecretKey::GeminiApiKey)?;
        delete_secret_at_path(&self.secrets_path, SecretKey::ElevenLabsApiKey)?;
        Ok(())
    }

    pub async fn prepare_session_context(&self) -> anyhow::Result<PreparedSessionContext> {
        let prepare_started = Instant::now();
        let cfg = self.config_store.load()?;
        let app = self.ctx.foreground_app().await?;
        let effective_config = resolve_effective_config(
            &cfg.defaults,
            &cfg.profiles,
            &app,
            &EphemeralOverrides::default(),
        );

        let llm_api_key = self.load_llm_api_key(&effective_config.llm_provider_kind)?;
        let requested_visual_dispatch = resolve_visual_context_dispatch(
            effective_config.context.visual_context_mode,
            &effective_config.llm_provider_kind,
            &effective_config.llm_api_kind,
        );
        let preflight_llm_api_key = llm_api_key.clone();
        let preflight_guard = should_preflight_llm(&cfg, &effective_config, &llm_api_key)
            .then(|| {
                let provider = self.llm_provider.clone();
                let provider_kind = effective_config.llm_provider_kind.clone();
                let base_url = effective_config.llm_base_url.clone();
                let preflight_delay_ms = effective_config.llm_preflight_delay_ms;
                let handle = tokio::spawn(async move {
                    if preflight_delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(preflight_delay_ms)).await;
                    }

                    let started = std::time::Instant::now();
                    match provider
                        .preflight_http_connect(&provider_kind, &base_url, &preflight_llm_api_key)
                        .await
                    {
                        Ok(()) => {
                            log::debug!(
                                "llm preflight completed: mode=http_connect delay_ms={} elapsed_ms={}",
                                preflight_delay_ms,
                                started.elapsed().as_millis()
                            );
                        }
                        Err(e) => {
                            log::debug!(
                                "llm preflight failed: mode=http_connect delay_ms={} elapsed_ms={} err={e}",
                                preflight_delay_ms,
                                started.elapsed().as_millis()
                            );
                        }
                    }
                });
                PreflightAbortGuard::new(handle.abort_handle())
            });

        let mut context = self
            .snapshot_context_with_effective_policy(&self.ctx, &effective_config)
            .await;
        log::debug!(
            "prepared session context captured: provider_kind={} visual_mode={:?} requested_dispatch={:?} capture_scope={} capture_actual_scope={:?} screenshot_capture_elapsed_ms={:?} capture_fallback_reason={:?} screenshot={} elapsed_ms={}",
            effective_config.llm_provider_kind,
            effective_config.context.visual_context_mode,
            requested_visual_dispatch,
            visual_capture_scope_label(effective_config.context.visual_capture_scope),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.actual_scope),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.capture_elapsed_ms),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.fallback_reason.as_deref()),
            context.screenshot.is_some(),
            prepare_started.elapsed().as_millis()
        );

        if matches!(requested_visual_dispatch, VisualContextDispatch::Ocr) {
            match context.screenshot.as_ref() {
                Some(screenshot) if !llm_api_key.trim().is_empty() => {
                    match self
                        .extract_screen_ocr_text(&effective_config, &llm_api_key, screenshot)
                        .await
                    {
                        Ok(ocr) if !ocr.text.trim().is_empty() => {
                            let text_chars = ocr.text.chars().count();
                            log::debug!(
                                "prepared session visual ocr cached: provider_kind={} api_kind={} capture_scope={} elapsed_ms={} chars={} first_token_ms={:?}",
                                effective_config.llm_provider_kind,
                                effective_config.llm_api_kind,
                                visual_capture_scope_label(
                                    effective_config.context.visual_capture_scope
                                ),
                                ocr.elapsed_ms,
                                text_chars,
                                ocr.first_token_ms
                            );
                            context.precomputed_screen_ocr = Some(PreparedScreenOcr {
                                text: ocr.text,
                                elapsed_ms: ocr.elapsed_ms,
                                first_token_ms: ocr.first_token_ms,
                            });
                        }
                        Ok(_) => {
                            log::debug!(
                                "prepared session visual ocr produced empty text; stop path will retry OCR if needed"
                            );
                        }
                        Err(e) => {
                            log::warn!(
                                "prepared session visual ocr failed; stop path will retry OCR if needed: {e}"
                            );
                        }
                    }
                }
                Some(_) => {
                    log::debug!(
                        "prepared session visual ocr skipped because no LLM API key is configured"
                    );
                }
                None => {
                    log::debug!(
                        "prepared session visual ocr skipped because no screenshot artifact was captured"
                    );
                }
            }
        }
        log::debug!(
            "prepared session context ready: provider_kind={} visual_mode={:?} requested_dispatch={:?} capture_scope={} capture_actual_scope={:?} screenshot_capture_elapsed_ms={:?} capture_fallback_reason={:?} screenshot={} precomputed_screen_ocr={} elapsed_ms={}",
            effective_config.llm_provider_kind,
            effective_config.context.visual_context_mode,
            requested_visual_dispatch,
            visual_capture_scope_label(effective_config.context.visual_capture_scope),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.actual_scope),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.capture_elapsed_ms),
            context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.fallback_reason.as_deref()),
            context.screenshot.is_some(),
            context.precomputed_screen_ocr.is_some(),
            prepare_started.elapsed().as_millis()
        );

        Ok(PreparedSessionContext {
            cfg,
            app,
            context,
            effective_config,
            preflight_abort: preflight_guard.map(PreflightAbortGuard::into_abort_handle),
        })
    }

    pub async fn get_foreground_app(&self) -> anyhow::Result<voicewin_core::types::AppIdentity> {
        self.ctx.foreground_app().await
    }

    pub async fn capture_context_snapshot(&self) -> ContextSnapshot {
        self.ctx.snapshot_context().await.unwrap_or_default()
    }

    pub async fn preview_prompt(
        &self,
        prompt: PromptTemplate,
        transcript: String,
        forced_profile_id: Option<ProfileId>,
        force_defaults: bool,
    ) -> anyhow::Result<PromptPreviewResponse> {
        let app = match self.ctx.foreground_app().await {
            Ok(app) => app,
            Err(e) => {
                log::warn!("preview_prompt could not capture foreground app; using defaults: {e}");
                AppIdentity::new()
            }
        };
        let cfg = self.config_store.load()?;
        let ephemeral = EphemeralOverrides {
            forced_profile_id: forced_profile_id.clone(),
            ..EphemeralOverrides::default()
        };
        let profiles = if force_defaults {
            &[][..]
        } else {
            &cfg.profiles[..]
        };
        let effective = resolve_effective_config(&cfg.defaults, profiles, &app, &ephemeral);
        let snapshot = self
            .ctx
            .snapshot_context_for_policy(screenshot_capture_options_for_effective(&effective))
            .await
            .unwrap_or_default();
        self.preview_prompt_with_loaded_source(
            cfg,
            prompt,
            transcript,
            app,
            snapshot,
            forced_profile_id,
            force_defaults,
        )
        .await
    }

    pub async fn probe_llm_provider(
        &self,
        provider_kind: &str,
        api_kind: &str,
        base_url: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        probe_kind: ProviderProbeKind,
    ) -> anyhow::Result<ProviderProbeResponse> {
        let llm_api_key = self.load_llm_api_key(provider_kind)?;
        if llm_api_key.trim().is_empty() {
            anyhow::bail!("no API key configured for provider kind: {provider_kind}");
        }
        if base_url.trim().is_empty() {
            anyhow::bail!("LLM base URL is empty");
        }
        if model.trim().is_empty() {
            anyhow::bail!("LLM model is empty");
        }

        let (
            system_message,
            user_message,
            attached_image,
            prompt_mode,
            transcript,
            expected_output,
        ) = match probe_kind {
            ProviderProbeKind::Smoke => (
                "Return exactly: VoiceWin provider probe ok.".to_string(),
                "<PROBE>\nRespond with exactly: VoiceWin provider probe ok.\n</PROBE>".to_string(),
                None,
                PromptMode::Assistant,
                String::new(),
                PROVIDER_PROBE_SMOKE_OUTPUT.to_string(),
            ),
            ProviderProbeKind::ScreenshotProductName => {
                if !llm_supports_attached_images(provider_kind, api_kind) {
                    anyhow::bail!(
                        "{}",
                        screenshot_context_warning(provider_kind, api_kind).unwrap_or_else(|| {
                            "Configured LLM API does not support screenshot probe.".into()
                        })
                    );
                }

                let prompt = PromptTemplate {
                        id: PromptId::new(),
                        title: "Provider Screenshot Probe".into(),
                        mode: PromptMode::Enhancer,
                        prompt_text: "Correct the dictated product name using the attached screenshot if needed. Return only the corrected product name as plain text. No explanation.".into(),
                        trigger_words: vec![],
                    };
                let transcript = "voice wen".to_string();
                let context = EnhancementContext {
                    screenshot: Some(voicewin_core::context::ImageArtifact {
                        data_url: PROVIDER_PROBE_SCREENSHOT_DATA_URL.trim().to_string(),
                    }),
                    ..Default::default()
                };
                let built = build_enhancement_prompt(&transcript, &prompt, &context);
                (
                    built.system_message,
                    built.user_message,
                    context.screenshot,
                    prompt.mode,
                    transcript,
                    PROVIDER_PROBE_SCREENSHOT_OUTPUT.to_string(),
                )
            }
        };

        let started = Instant::now();
        let raw = self
            .llm_provider
            .enhance(
                provider_kind,
                api_kind,
                base_url,
                &llm_api_key,
                model,
                reasoning_effort,
                &system_message,
                &user_message,
                attached_image.as_ref(),
            )
            .await?;
        let final_output = post_process_llm_output(&raw.text, prompt_mode, &transcript);
        let elapsed_ms = elapsed_ms_floor_one(started);

        Ok(ProviderProbeResponse {
            probe_kind,
            elapsed_ms,
            first_token_ms: raw.first_token_ms,
            input_tokens: raw.input_tokens,
            cached_input_tokens: raw.cached_input_tokens,
            provider_kind: provider_kind.to_string(),
            api_kind: api_kind.to_string(),
            model: model.to_string(),
            expected_output,
            final_output: final_output.text,
            warning: final_output.warning,
        })
    }

    pub async fn preview_prompt_with_app_snapshot(
        &self,
        prompt: PromptTemplate,
        transcript: String,
        app: AppIdentity,
        snapshot: ContextSnapshot,
        forced_profile_id: Option<ProfileId>,
        force_defaults: bool,
    ) -> anyhow::Result<PromptPreviewResponse> {
        let cfg = self.config_store.load()?;
        self.preview_prompt_with_loaded_source(
            cfg,
            prompt,
            transcript,
            app,
            snapshot,
            forced_profile_id,
            force_defaults,
        )
        .await
    }

    pub async fn run_session(
        &self,
        req: RunSessionRequest,
        audio: AudioInput,
    ) -> anyhow::Result<RunSessionResponse> {
        self.run_session_with_hook(req, audio, |_stage| async {})
            .await
    }

    pub async fn run_session_with_hook<F, Fut>(
        &self,
        req: RunSessionRequest,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<RunSessionResponse>
    where
        F: Fn(&'static str) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        let cfg = self.config_store.load()?;
        self.run_session_with_loaded_config(cfg, req, audio, on_stage)
            .await
    }

    pub async fn run_session_with_hook_using_config<F, Fut>(
        &self,
        cfg: AppConfig,
        req: RunSessionRequest,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<RunSessionResponse>
    where
        F: Fn(&'static str) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        self.run_session_with_loaded_config(cfg, req, audio, on_stage)
            .await
    }

    pub async fn run_session_with_prepared_with_hook<F, Fut>(
        &self,
        prepared: PreparedSessionContext,
        req: RunSessionRequest,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<RunSessionResponse>
    where
        F: Fn(&'static str) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        prepared.abort_preflight();
        let ctx = FixedAppContextProvider::new(prepared.app, prepared.context).boxed();
        self.run_session_with_context(prepared.cfg, ctx, req, audio, on_stage)
            .await
    }

    async fn run_session_with_loaded_config<F, Fut>(
        &self,
        cfg: AppConfig,
        req: RunSessionRequest,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<RunSessionResponse>
    where
        F: Fn(&'static str) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        self.run_session_with_context(cfg, self.ctx.clone(), req, audio, on_stage)
            .await
    }

    async fn run_session_with_context<F, Fut>(
        &self,
        cfg: AppConfig,
        ctx: Arc<dyn AppContextProvider>,
        req: RunSessionRequest,
        audio: AudioInput,
        on_stage: F,
    ) -> anyhow::Result<RunSessionResponse>
    where
        F: Fn(&'static str) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
    {
        // Split request fields so we can move transcript into the engine call.
        let RunSessionRequest {
            transcript,
            warning: request_warning,
        } = req;

        // Design-draft UI treats History as always enabled.
        // Keep the config flag for backward compatibility, but it must not disable history.
        let history_enabled = true;
        let _ = cfg.defaults.history_enabled;

        let ctx = self.wrap_context_provider(ctx);
        let openai_api_key = self.load_openai_api_key()?;
        let gemini_api_key = self.load_gemini_api_key()?;
        let elevenlabs_api_key = self.load_elevenlabs_api_key()?;
        let llm: Arc<dyn LlmProvider> = self.llm_provider.clone();
        let engine: VoicewinEngine = build_engine_from_config(
            cfg,
            ctx.clone(),
            self.inserter.clone(),
            llm,
            openai_api_key,
            gemini_api_key,
            elevenlabs_api_key,
        )
        .await?;

        // Run the full session pipeline and emit stage progress.
        // If `req.transcript` is provided, skip STT and run from the given transcript.
        let res = if transcript.trim().is_empty() {
            engine.run_session_with_hook(audio, on_stage).await
        } else {
            engine
                .run_session_with_transcript_with_hook(transcript, on_stage)
                .await
        };

        let mut session_error = None::<String>;
        let mut session_result = match res {
            Ok(result) => Some(result),
            Err(e) => {
                log::debug!("session execution failed before structured result: {e}");
                session_error = Some(e.to_string());
                None
            }
        };

        let (stage, final_text, mut warning, error) = match session_result.as_ref() {
            Some(result) => {
                let stage = result
                    .stage_label
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", result.stage).to_lowercase());
                (
                    stage,
                    result.final_text.clone(),
                    result.warning.clone(),
                    result.error.clone(),
                )
            }
            None => ("error".into(), None, None, session_error.clone()),
        };

        // Attach any extra warning requested by the caller.
        if let Some(w) = request_warning.as_ref().filter(|s| !s.trim().is_empty()) {
            merge_message(&mut warning, w);
        }
        if let Some(result) = session_result.as_mut() {
            result.warning = warning.clone();
            result.error = error.clone();
        }

        if history_enabled {
            let should_write = final_text
                .as_ref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
                || warning.is_some()
                || error.is_some();

            if should_write {
                let text = final_text.clone().unwrap_or_default();
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                let entry = if let Some(result) = session_result.as_ref() {
                    voicewin_runtime::history::HistoryEntry {
                        id: String::new(),
                        ts_unix_ms: ts,
                        app_process_name: result.app.process_name.as_ref().map(|p| p.0.clone()),
                        app_exe_path: result.app.exe_path.as_ref().map(|p| p.0.clone()),
                        app_window_title: result.app.window_title.as_ref().map(|t| t.0.clone()),
                        text,
                        raw_transcript: result.transcript.as_ref().map(|t| t.text.clone()),
                        enhanced_text: result.enhanced.as_ref().map(|t| t.text.clone()),
                        prompt_id: result.prompt_id.clone(),
                        prompt_title: result.prompt_title.clone(),
                        matched_profile_name: result.config.matched_profile_name.clone(),
                        detected_trigger_word: result.detected_trigger_word.clone(),
                        stt_provider: result.transcript.as_ref().map(|t| t.provider.clone()),
                        stt_model: result.transcript.as_ref().map(|t| t.model.clone()),
                        llm_provider: result.enhanced.as_ref().map(|t| t.provider.clone()),
                        llm_model: result.enhanced.as_ref().map(|t| t.model.clone()),
                        transcription_ms: result.timings.transcription_ms,
                        enhancement_ms: result.timings.enhancement_ms,
                        enhancement_first_token_ms: result.timings.enhancement_first_token_ms,
                        enhancement_input_tokens: result
                            .enhanced
                            .as_ref()
                            .and_then(|t| t.input_tokens),
                        enhancement_cached_input_tokens: result
                            .enhanced
                            .as_ref()
                            .and_then(|t| t.cached_input_tokens),
                        context_flags: Some(result.config.context.clone()),
                        visual_context_runtime: Some(result.visual_context.clone()),
                        stage: stage.clone(),
                        warning: warning.clone(),
                        error: error.clone(),
                    }
                } else {
                    let app = ctx.foreground_app().await.ok();
                    voicewin_runtime::history::HistoryEntry {
                        id: String::new(),
                        ts_unix_ms: ts,
                        app_process_name: app
                            .as_ref()
                            .and_then(|a| a.process_name.as_ref())
                            .map(|p| p.0.clone()),
                        app_exe_path: app
                            .as_ref()
                            .and_then(|a| a.exe_path.as_ref())
                            .map(|p| p.0.clone()),
                        app_window_title: app
                            .as_ref()
                            .and_then(|a| a.window_title.as_ref())
                            .map(|t| t.0.clone()),
                        text,
                        raw_transcript: None,
                        enhanced_text: None,
                        prompt_id: None,
                        prompt_title: None,
                        matched_profile_name: None,
                        detected_trigger_word: None,
                        stt_provider: None,
                        stt_model: None,
                        llm_provider: None,
                        llm_model: None,
                        transcription_ms: None,
                        enhancement_ms: None,
                        enhancement_first_token_ms: None,
                        enhancement_input_tokens: None,
                        enhancement_cached_input_tokens: None,
                        context_flags: None,
                        visual_context_runtime: None,
                        stage: stage.clone(),
                        warning: warning.clone(),
                        error: error.clone(),
                    }
                };

                // Best-effort: write history alongside config.
                let history_path = self
                    .config_store
                    .path()
                    .parent()
                    .map(|p| p.join("history.json"))
                    .unwrap_or_else(|| PathBuf::from("history.json"));

                let store = voicewin_runtime::history::HistoryStore::at_path(history_path);
                if let Err(e) = store.append(entry) {
                    log::error!("failed to append history: {e}");
                }
            }
        }

        Ok(RunSessionResponse {
            stage,
            final_text,
            warning,
            error,
        })
    }

    fn custom_vocabulary_path(&self) -> PathBuf {
        self.config_store
            .path()
            .parent()
            .map(|p| p.join("custom_vocabulary.txt"))
            .unwrap_or_else(|| PathBuf::from("custom_vocabulary.txt"))
    }

    fn load_custom_vocabulary_source(&self) -> anyhow::Result<Option<String>> {
        load_custom_vocabulary_file(&self.custom_vocabulary_path())
    }

    fn wrap_context_provider(
        &self,
        ctx: Arc<dyn AppContextProvider>,
    ) -> Arc<dyn AppContextProvider> {
        let custom_vocabulary = match self.load_custom_vocabulary_source() {
            Ok(custom_vocabulary) => custom_vocabulary,
            Err(e) => {
                log::warn!("failed to load custom vocabulary source: {e}");
                return ctx;
            }
        };
        if custom_vocabulary.is_none() {
            return ctx;
        }
        VocabularyOverlayContextProvider::new(ctx, custom_vocabulary).boxed()
    }

    async fn snapshot_context_with_effective_policy(
        &self,
        ctx: &Arc<dyn AppContextProvider>,
        effective: &EffectiveConfig,
    ) -> ContextSnapshot {
        let snapshot = ctx
            .snapshot_context_for_policy(screenshot_capture_options_for_effective(effective))
            .await
            .unwrap_or_default();
        match self.load_custom_vocabulary_source() {
            Ok(custom_vocabulary) => merge_custom_vocabulary(snapshot, custom_vocabulary),
            Err(e) => {
                log::warn!("failed to load custom vocabulary source: {e}");
                snapshot
            }
        }
    }

    fn load_openai_api_key(&self) -> anyhow::Result<String> {
        Ok(
            get_secret_at_path(&self.secrets_path, SecretKey::OpenAiCompatibleApiKey)?
                .unwrap_or_default(),
        )
    }

    fn load_gemini_api_key(&self) -> anyhow::Result<String> {
        Ok(get_secret_at_path(&self.secrets_path, SecretKey::GeminiApiKey)?.unwrap_or_default())
    }

    fn load_elevenlabs_api_key(&self) -> anyhow::Result<String> {
        Ok(
            get_secret_at_path(&self.secrets_path, SecretKey::ElevenLabsApiKey)?
                .unwrap_or_default(),
        )
    }

    fn load_llm_api_key(&self, provider_kind: &str) -> anyhow::Result<String> {
        match provider_kind.trim() {
            "gemini" => self.load_gemini_api_key(),
            "openai_compatible" | "" => self.load_openai_api_key(),
            other => Err(anyhow::anyhow!("unsupported llm provider kind: {other}")),
        }
    }

    async fn extract_screen_ocr_text(
        &self,
        effective: &EffectiveConfig,
        llm_api_key: &str,
        screenshot: &voicewin_core::context::ImageArtifact,
    ) -> anyhow::Result<ScreenOcrResult> {
        let Some(ocr_api_kind) =
            ocr_sidecar_api_kind(&effective.llm_provider_kind, &effective.llm_api_kind)
        else {
            anyhow::bail!(
                "no OCR-capable API mapping for provider kind: {}",
                effective.llm_provider_kind
            );
        };

        let started = Instant::now();
        log::debug!(
            "visual context ocr start: provider_kind={} selected_api_kind={} ocr_api_kind={} model={} capture_scope={} screenshot_bytes={}",
            effective.llm_provider_kind,
            effective.llm_api_kind,
            ocr_api_kind,
            effective.llm_model,
            visual_capture_scope_label(effective.context.visual_capture_scope),
            screenshot.data_url.len()
        );
        let response = match self
            .llm_provider
            .enhance(
                &effective.llm_provider_kind,
                ocr_api_kind,
                &effective.llm_base_url,
                llm_api_key,
                &effective.llm_model,
                effective.llm_reasoning_effort.as_deref(),
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
                    effective.llm_provider_kind,
                    ocr_api_kind,
                    started.elapsed().as_millis(),
                    error
                );
                return Err(error);
            }
        };
        let text = response.text.trim().to_string();
        let elapsed_ms = elapsed_ms_floor_one(started);
        log::debug!(
            "visual context ocr done: provider_kind={} ocr_api_kind={} elapsed_ms={} chars={} first_token_ms={:?}",
            effective.llm_provider_kind,
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

    async fn preview_prompt_with_loaded_source(
        &self,
        cfg: AppConfig,
        prompt: PromptTemplate,
        transcript: String,
        app: AppIdentity,
        snapshot: ContextSnapshot,
        forced_profile_id: Option<ProfileId>,
        force_defaults: bool,
    ) -> anyhow::Result<PromptPreviewResponse> {
        let ephemeral = EphemeralOverrides {
            forced_profile_id,
            ..EphemeralOverrides::default()
        };
        let profiles = if force_defaults {
            &[][..]
        } else {
            &cfg.profiles[..]
        };
        let effective = resolve_effective_config(&cfg.defaults, profiles, &app, &ephemeral);
        let llm_api_key = self.load_llm_api_key(&effective.llm_provider_kind)?;

        if llm_api_key.trim().is_empty() {
            anyhow::bail!(
                "no API key configured for provider kind: {}",
                effective.llm_provider_kind
            );
        }
        if effective.llm_base_url.trim().is_empty() {
            anyhow::bail!("LLM base URL is empty");
        }
        if effective.llm_model.trim().is_empty() {
            anyhow::bail!("LLM model is empty");
        }
        if transcript.trim().is_empty() {
            anyhow::bail!("preview transcript is empty");
        }

        let snapshot = match self.load_custom_vocabulary_source() {
            Ok(custom_vocabulary) => merge_custom_vocabulary(snapshot, custom_vocabulary),
            Err(e) => {
                log::warn!("failed to load custom vocabulary source: {e}");
                snapshot
            }
        };
        let visual_dispatch = resolve_visual_context_dispatch(
            effective.context.visual_context_mode,
            &effective.llm_provider_kind,
            &effective.llm_api_kind,
        );
        let mut visual_context_runtime = VisualContextRuntime {
            mode: effective.context.visual_context_mode,
            capture_scope: effective.context.visual_capture_scope,
            ..Default::default()
        };
        apply_screenshot_capture_metadata(&mut visual_context_runtime, &snapshot);
        log::debug!(
            "preview visual context dispatch: mode={:?} requested_dispatch={:?} provider_kind={} api_kind={} capture_scope={} capture_actual_scope={:?} screenshot_capture_elapsed_ms={:?} capture_fallback_reason={:?} screenshot_present={}",
            effective.context.visual_context_mode,
            visual_dispatch,
            effective.llm_provider_kind,
            effective.llm_api_kind,
            visual_capture_scope_label(effective.context.visual_capture_scope),
            visual_context_runtime.capture_actual_scope,
            visual_context_runtime.screenshot_capture_elapsed_ms,
            visual_context_runtime.capture_fallback_reason.as_deref(),
            snapshot.screenshot.is_some()
        );
        let mut warning = screenshot_context_warning_for_effective(&effective);
        let (screen_ocr_result, screen_ocr_source) = if matches!(
            visual_dispatch,
            VisualContextDispatch::Ocr
        ) {
            if let Some(precomputed) = snapshot
                .precomputed_screen_ocr
                .as_ref()
                .and_then(screen_ocr_result_from_precomputed)
            {
                log::debug!(
                    "preview visual context using precomputed OCR from snapshot: elapsed_ms={} chars={} first_token_ms={:?}",
                    precomputed.elapsed_ms,
                    precomputed.text.chars().count(),
                    precomputed.first_token_ms
                );
                (Some(precomputed), Some(ScreenOcrSource::Prepared))
            } else {
                match snapshot.screenshot.as_ref() {
                    Some(screenshot) => match self
                        .extract_screen_ocr_text(&effective, &llm_api_key, screenshot)
                        .await
                    {
                        Ok(ocr) => (Some(ocr), Some(ScreenOcrSource::Inline)),
                        Err(e) => {
                            merge_message(
                                &mut warning,
                                format!("Visual OCR failed; continuing without OCR text. ({e})"),
                            );
                            (None, None)
                        }
                    },
                    None => {
                        log::debug!(
                            "preview visual context ocr requested but no screenshot artifact or precomputed OCR was available"
                        );
                        (None, None)
                    }
                }
            }
        } else {
            (None, None)
        };
        if let Some(ocr) = screen_ocr_result.as_ref() {
            visual_context_runtime.screen_ocr_source = screen_ocr_source;
            visual_context_runtime.screen_ocr_elapsed_ms = Some(ocr.elapsed_ms);
            visual_context_runtime.screen_ocr_first_token_ms = ocr.first_token_ms;
            visual_context_runtime.screen_ocr_text_chars =
                Some(ocr.text.chars().count().try_into().unwrap_or(u64::MAX));
        }
        let screen_ocr_text = match screen_ocr_result.as_ref() {
            Some(ocr) if !ocr.text.trim().is_empty() => Some(ocr.text.clone()),
            Some(_) => {
                log::debug!(
                    "preview visual context ocr produced empty text; continuing without OCR context"
                );
                None
            }
            None => None,
        };
        let attached_screenshot = if matches!(visual_dispatch, VisualContextDispatch::Screenshot) {
            snapshot.screenshot.clone()
        } else {
            None
        };
        if matches!(visual_dispatch, VisualContextDispatch::Screenshot)
            && attached_screenshot.is_none()
        {
            if let Some(msg) = visual_context_capture_unavailable_warning(
                visual_dispatch,
                effective.context.visual_capture_scope,
            ) {
                merge_message(&mut warning, msg);
            }
        } else if matches!(visual_dispatch, VisualContextDispatch::Ocr)
            && screen_ocr_text.is_none()
            && snapshot.screenshot.is_none()
            && snapshot.precomputed_screen_ocr.is_none()
        {
            if let Some(msg) = visual_context_capture_unavailable_warning(
                visual_dispatch,
                effective.context.visual_capture_scope,
            ) {
                merge_message(&mut warning, msg);
            }
        }
        visual_context_runtime.dispatch = if attached_screenshot.is_some() {
            VisualContextDispatch::Screenshot
        } else if screen_ocr_text.is_some() {
            VisualContextDispatch::Ocr
        } else {
            VisualContextDispatch::Off
        };
        log::debug!(
            "preview visual context final: mode={:?} requested_dispatch={:?} actual_dispatch={:?} provider_kind={} api_kind={} capture_scope={} capture_actual_scope={:?} screenshot_capture_elapsed_ms={:?} capture_fallback_reason={:?} screenshot_attached={} screen_ocr_source={:?} screen_ocr_elapsed_ms={:?} screen_ocr_text_chars={:?}",
            effective.context.visual_context_mode,
            visual_dispatch,
            visual_context_runtime.dispatch,
            effective.llm_provider_kind,
            effective.llm_api_kind,
            visual_capture_scope_label(effective.context.visual_capture_scope),
            visual_context_runtime.capture_actual_scope,
            visual_context_runtime.screenshot_capture_elapsed_ms,
            visual_context_runtime.capture_fallback_reason.as_deref(),
            attached_screenshot.is_some(),
            visual_context_runtime.screen_ocr_source,
            visual_context_runtime.screen_ocr_elapsed_ms,
            visual_context_runtime.screen_ocr_text_chars
        );
        let context = EnhancementContext {
            clipboard_context: effective
                .context
                .use_clipboard
                .then(|| snapshot.clipboard.clone())
                .flatten(),
            currently_selected_text: effective
                .context
                .use_selected_text
                .then(|| snapshot.selected_text.clone())
                .flatten(),
            current_window_context: effective
                .context
                .use_window_context
                .then(|| snapshot.window_context.clone())
                .flatten(),
            custom_vocabulary: effective
                .context
                .use_custom_vocabulary
                .then(|| snapshot.custom_vocabulary.clone())
                .flatten(),
            screen_ocr_text,
            screenshot: attached_screenshot.clone(),
        };

        let built = build_enhancement_prompt(&transcript, &prompt, &context);
        let started = Instant::now();
        let raw = self
            .llm_provider
            .enhance(
                &effective.llm_provider_kind,
                &effective.llm_api_kind,
                &effective.llm_base_url,
                &llm_api_key,
                &effective.llm_model,
                effective.llm_reasoning_effort.as_deref(),
                &built.system_message,
                &built.user_message,
                context.screenshot.as_ref(),
            )
            .await?;
        let final_output = post_process_llm_output_with_screen_ocr(
            &raw.text,
            prompt.mode.clone(),
            &transcript,
            context.screen_ocr_text.as_deref(),
        );
        if let Some(extra) = final_output.warning.as_deref() {
            merge_message(&mut warning, extra);
        }
        let elapsed_ms = elapsed_ms_floor_one(started);

        Ok(PromptPreviewResponse {
            elapsed_ms,
            first_token_ms: raw.first_token_ms,
            input_tokens: raw.input_tokens,
            cached_input_tokens: raw.cached_input_tokens,
            visual_context_runtime: Some(visual_context_runtime),
            app_process_name: app.process_name.as_ref().map(|value| value.0.clone()),
            app_window_title: app.window_title.as_ref().map(|value| value.0.clone()),
            matched_profile_name: effective.matched_profile_name.clone(),
            provider_kind: effective.llm_provider_kind.clone(),
            api_kind: effective.llm_api_kind.clone(),
            model: effective.llm_model.clone(),
            system_message: built.system_message,
            user_message: built.user_message,
            raw_output: raw.text,
            final_output: final_output.text,
            warning,
        })
    }
}

fn should_preflight_llm(cfg: &AppConfig, effective: &EffectiveConfig, llm_api_key: &str) -> bool {
    if effective.llm_preflight_mode != "http_connect" {
        return false;
    }

    if llm_api_key.trim().is_empty()
        || effective.llm_base_url.trim().is_empty()
        || effective.llm_model.trim().is_empty()
        || cfg.prompts.is_empty()
    {
        return false;
    }

    effective.enable_enhancement
        || effective.prompt_id.is_some()
        || cfg
            .prompts
            .iter()
            .any(|prompt| !prompt.trigger_words.is_empty())
}

const CUSTOM_VOCABULARY_MAX_BYTES: usize = 32 * 1024;

fn load_custom_vocabulary_file(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(path)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("read custom vocabulary file: {}", path.display()))?;

    Ok(normalize_custom_vocabulary_text(&raw))
}

fn normalize_custom_vocabulary_text(raw: &str) -> Option<String> {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    let mut total = 0usize;

    for raw_line in raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if !seen.insert(line.to_string()) {
            continue;
        }

        let extra = if lines.is_empty() {
            line.len()
        } else {
            line.len() + 1
        };
        if total.saturating_add(extra) > CUSTOM_VOCABULARY_MAX_BYTES {
            break;
        }

        total += extra;
        lines.push(line.to_string());
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn merge_custom_vocabulary(
    mut snapshot: ContextSnapshot,
    file_custom_vocabulary: Option<String>,
) -> ContextSnapshot {
    match (
        snapshot.custom_vocabulary.take(),
        file_custom_vocabulary.filter(|value| !value.trim().is_empty()),
    ) {
        (Some(existing), Some(from_file)) => {
            snapshot.custom_vocabulary =
                normalize_custom_vocabulary_text(&format!("{existing}\n{from_file}"));
        }
        (Some(existing), None) => {
            snapshot.custom_vocabulary = normalize_custom_vocabulary_text(&existing);
        }
        (None, Some(from_file)) => {
            snapshot.custom_vocabulary = Some(from_file);
        }
        (None, None) => {
            snapshot.custom_vocabulary = None;
        }
    }

    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use voicewin_core::enhancement::{PromptMode, PromptTemplate};
    use voicewin_core::power_mode::{
        AppMatcher, GlobalDefaults, PowerModeOverrides, PowerModeProfile,
    };
    use voicewin_core::types::{InsertMode, ProfileId, PromptId};
    use voicewin_engine::traits::{AppContextProvider, Inserter};
    use voicewin_platform::test::MemoryInserter;
    use voicewin_runtime::history::HistoryStore;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct FailingInserter;

    #[async_trait]
    impl Inserter for FailingInserter {
        async fn insert(&self, _text: &str, _mode: InsertMode) -> anyhow::Result<()> {
            anyhow::bail!("simulated insert failure")
        }
    }

    #[derive(Debug)]
    struct MutableContextProvider {
        state: std::sync::Mutex<(
            AppIdentity,
            ContextSnapshot,
            Option<voicewin_core::context::ImageArtifact>,
            Option<voicewin_core::context::ScreenshotCaptureMetadata>,
        )>,
        screenshot_captures: std::sync::atomic::AtomicUsize,
        screenshot_options:
            std::sync::Mutex<Vec<voicewin_engine::traits::ScreenshotCaptureOptions>>,
    }

    impl MutableContextProvider {
        fn new(app: AppIdentity, context: ContextSnapshot) -> Self {
            Self {
                state: std::sync::Mutex::new((app, context, None, None)),
                screenshot_captures: std::sync::atomic::AtomicUsize::new(0),
                screenshot_options: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn set(&self, app: AppIdentity, context: ContextSnapshot) {
            let mut state = self.state.lock().unwrap();
            state.0 = app;
            state.1 = context;
        }

        fn set_captured_screenshot(
            &self,
            screenshot: Option<voicewin_core::context::ImageArtifact>,
        ) {
            self.state.lock().unwrap().2 = screenshot;
        }

        fn set_captured_screenshot_metadata(
            &self,
            metadata: Option<voicewin_core::context::ScreenshotCaptureMetadata>,
        ) {
            self.state.lock().unwrap().3 = metadata;
        }

        fn screenshot_capture_count(&self) -> usize {
            self.screenshot_captures
                .load(std::sync::atomic::Ordering::SeqCst)
        }

        fn last_screenshot_options(
            &self,
        ) -> Vec<voicewin_engine::traits::ScreenshotCaptureOptions> {
            self.screenshot_options.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AppContextProvider for MutableContextProvider {
        async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
            Ok(self.state.lock().unwrap().0.clone())
        }

        async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
            Ok(self.state.lock().unwrap().1.clone())
        }

        async fn capture_screenshot(
            &self,
            options: ScreenshotCaptureOptions,
        ) -> anyhow::Result<Option<CapturedScreenshot>> {
            self.screenshot_captures
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.screenshot_options.lock().unwrap().push(options);
            let state = self.state.lock().unwrap();
            Ok(state.2.clone().map(|image| CapturedScreenshot {
                image,
                metadata: state.3.clone().unwrap_or(
                    voicewin_core::context::ScreenshotCaptureMetadata {
                        actual_scope: Some(options.scope),
                        capture_elapsed_ms: None,
                        fallback_reason: None,
                    },
                ),
            }))
        }
    }

    #[test]
    fn microphone_change_is_detected() {
        assert!(microphone_device_changed(
            Some("Built-in Mic"),
            Some("USB Audio")
        ));
    }

    #[test]
    fn microphone_id_change_is_detected_even_when_name_matches() {
        assert!(microphone_selection_changed(
            Some("cpal:1:USB Mic"),
            Some("USB Mic"),
            Some("cpal:2:USB Mic"),
            Some("USB Mic"),
        ));
    }

    #[test]
    fn unchanged_microphone_is_not_detected_as_change() {
        assert!(!microphone_device_changed(
            Some("Built-in Mic"),
            Some("Built-in Mic")
        ));
    }

    #[test]
    fn empty_names_are_treated_as_none() {
        assert!(!microphone_device_changed(Some(" "), None));
        assert!(microphone_device_changed(None, Some("USB Audio")));
    }

    #[tokio::test]
    async fn service_roundtrip_and_run_session_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = voicewin_platform::test::TestContextProvider::new(
            voicewin_core::types::AppIdentity::new().with_process_name("slack.exe"),
            Default::default(),
        )
        .boxed();
        let inserter = Arc::new(voicewin_platform::test::StdoutInserter);

        let svc = AppService::new(config_path.clone(), ctx, inserter);

        // CI runners (and some dev machines) have no audio input device.
        // The service should still be constructible without panicking.
        #[cfg(windows)]
        {
            if svc.start_recording().await.is_err() {
                // Skip if audio is unavailable.
                return;
            }
            let _ = svc.cancel_recording().await;
        }

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec!["rewrite".into()],
            }],
            llm_api_key_present: false,
        };

        svc.save_config(&cfg).unwrap();
        let loaded = svc.load_config().unwrap();
        assert_eq!(loaded.defaults.language, "en");

        let audio = AudioInput {
            sample_rate_hz: 16_000,
            samples: vec![0.0; 160],
        };

        // This should not panic. It may fail (missing local model), but the service should
        // return a structured error instead of crashing.
        let _ = svc
            .run_session(
                RunSessionRequest {
                    transcript: "hi".into(),
                    warning: None,
                },
                audio,
            )
            .await;
    }

    #[tokio::test]
    async fn preview_prompt_returns_elapsed_ms_and_rendered_output() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\", Voicewin world.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(5))
                    .set_body_raw(sse, "text/event-stream"),
            )
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-preview").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Fix grammar.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: Some("low".into()),
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "hello voicewin world".into(), None, false)
            .await
            .unwrap();

        assert!(preview.elapsed_ms >= 1);
        assert!(preview.first_token_ms.is_some());
        assert_eq!(preview.app_process_name.as_deref(), Some("mail.exe"));
        assert_eq!(preview.app_window_title, None);
        assert_eq!(preview.matched_profile_name, None);
        assert_eq!(preview.provider_kind, "openai_compatible");
        assert_eq!(preview.api_kind, "responses_sse");
        assert_eq!(preview.model, "gpt-5.4");
        assert_eq!(preview.raw_output, "Hello, Voicewin world.");
        assert_eq!(preview.final_output, "Hello, Voicewin world.");
        assert_eq!(preview.warning, None);
        assert!(preview.system_message.contains("Fix grammar."));
        assert!(preview.user_message.contains("hello voicewin world"));
    }

    #[tokio::test]
    async fn preview_prompt_surfaces_post_process_warning() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"I'll send this as a clear follow-up:\\n\\n\\\"Please ship the VoiceWin update later this week.\\\"\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-preview-warning").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Fix grammar.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(
                prompt,
                "please ship the voice win update later this week".into(),
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(
            preview.final_output,
            "Please ship the VoiceWin update later this week."
        );
        assert_eq!(
            preview.warning.as_deref(),
            Some(
                "LLM output looked conversational; VoiceWin stripped assistant framing from the model output."
            )
        );
    }

    #[tokio::test]
    async fn preview_prompt_salvages_multimodal_screenshot_description_wrapper() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"The text in the image appears to be:\\n\\n**VoiceWin**\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                screenshot: Some(voicewin_core::context::ImageArtifact {
                    data_url: "data:image/png;base64,ZmFrZQ==".into(),
                }),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-preview-screenshot-wrapper")
            .unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text:
                "Correct the dictated product name using the attached screenshot if needed. Return only the corrected product name as plain text. No explanation."
                    .into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    visual_context_mode: voicewin_core::context::VisualContextMode::Screenshot,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                    ..Default::default()
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "voice wen".into(), None, false)
            .await
            .unwrap();

        assert_eq!(
            preview.raw_output,
            "The text in the image appears to be:\n\n**VoiceWin**"
        );
        assert_eq!(preview.final_output, "VoiceWin");
        assert_eq!(
            preview.warning.as_deref(),
            Some(
                "LLM output looked conversational; VoiceWin stripped assistant framing from the model output."
            )
        );
    }

    #[tokio::test]
    async fn preview_prompt_strips_instruction_echo_wrapper() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Turn this into a polished sentence: Hello, VoiceWin world.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-preview-instruction-echo")
            .unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Fix grammar.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(
                prompt,
                "turn this into a polished sentence: hello voicewin world".into(),
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(preview.final_output, "Hello, VoiceWin world.");
        assert_eq!(
            preview.warning.as_deref(),
            Some(
                "LLM output echoed transcript instruction framing; VoiceWin stripped the instruction wrapper from the model output."
            )
        );
    }

    #[tokio::test]
    async fn preview_prompt_uses_effective_profile_llm_config_and_context() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Profile\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" output.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                clipboard: Some("clipboard from profile".into()),
                window_context: Some("window context".into()),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-profile").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Profile rewrite.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.invalid/v1".into(),
                llm_model: "default-model".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "Mail".into(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("mail.exe".into())],
                overrides: PowerModeOverrides {
                    llm_base_url: Some(server.uri()),
                    llm_model: Some("profile-model".into()),
                    context: Some(voicewin_core::context::ContextToggles {
                        use_clipboard: true,
                        use_selected_text: false,
                        use_window_context: false,
                        use_custom_vocabulary: false,
                        visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                        visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                    }),
                    ..Default::default()
                },
            }],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "hello voicewin world".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.provider_kind, "openai_compatible");
        assert_eq!(preview.app_process_name.as_deref(), Some("mail.exe"));
        assert_eq!(preview.matched_profile_name.as_deref(), Some("Mail"));
        assert_eq!(preview.api_kind, "responses_sse");
        assert_eq!(preview.model, "profile-model");
        assert_eq!(preview.final_output, "Profile output.");
        assert!(preview.system_message.contains("<CLIPBOARD_CONTEXT>"));
        assert!(!preview.system_message.contains("<CURRENT_WINDOW_CONTEXT>"));

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/responses");
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(body.contains("Profile rewrite."));
        assert!(body.contains("<CLIPBOARD_CONTEXT>"));
        assert!(body.contains("clipboard from profile"));
    }

    #[tokio::test]
    async fn preview_prompt_includes_custom_vocabulary_from_file_when_enabled() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Custom\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" vocabulary.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            dir.path().join("custom_vocabulary.txt"),
            " VoiceWin \nElevenLabs\nVoiceWin\n",
        )
        .unwrap();

        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-custom-vocab").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Respect custom words.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: true,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "voicewin is nice".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "Custom vocabulary.");
        assert!(preview.system_message.contains("<CUSTOM_VOCABULARY>"));
        assert!(preview.system_message.contains("VoiceWin\nElevenLabs"));

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(body.contains("<CUSTOM_VOCABULARY>"));
        assert!(body.contains("VoiceWin"));
        assert!(body.contains("ElevenLabs"));
    }

    #[tokio::test]
    async fn preview_prompt_can_force_global_defaults_even_when_a_profile_matches() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Defaults\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" output.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                clipboard: Some("clipboard from profile".into()),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-defaults").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Defaults rewrite.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "default-model".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "Mail".into(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("mail.exe".into())],
                overrides: PowerModeOverrides {
                    llm_model: Some("profile-model".into()),
                    context: Some(voicewin_core::context::ContextToggles {
                        use_clipboard: true,
                        use_selected_text: false,
                        use_window_context: false,
                        use_custom_vocabulary: false,
                        visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                        visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                    }),
                    ..Default::default()
                },
            }],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "hello voicewin world".into(), None, true)
            .await
            .unwrap();

        assert_eq!(preview.matched_profile_name, None);
        assert_eq!(preview.model, "default-model");
        assert_eq!(preview.final_output, "Defaults output.");
        assert!(!preview.system_message.contains("<CLIPBOARD_CONTEXT>"));
    }

    #[tokio::test]
    async fn preview_prompt_includes_selected_text_when_enabled() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Selected\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" text.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                selected_text: Some("Current draft paragraph".into()),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-selected").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Rewrite relative to the selected text.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: true,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "make this sharper".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "Selected text.");
        assert!(preview.system_message.contains("<CURRENTLY_SELECTED_TEXT>"));
        assert!(preview.system_message.contains("Current draft paragraph"));

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(body.contains("<CURRENTLY_SELECTED_TEXT>"));
        assert!(body.contains("Current draft paragraph"));
    }

    #[tokio::test]
    async fn preview_prompt_attaches_screenshot_when_ocr_is_enabled() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Screenshot\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" output.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot_data_url = "data:image/png;base64,SGVsbG8=";
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                screenshot: Some(voicewin_core::context::ImageArtifact {
                    data_url: screenshot_data_url.into(),
                }),
                screenshot_metadata: Some(voicewin_core::context::ScreenshotCaptureMetadata {
                    actual_scope: Some(voicewin_core::context::VisualCaptureScope::Display),
                    capture_elapsed_ms: Some(19),
                    fallback_reason: Some("foreground_window_not_implemented".into()),
                }),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-screenshot").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use the screenshot when it helps.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Screenshot,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "read the screenshot".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "Screenshot output.");
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.mode),
            Some(voicewin_core::context::VisualContextMode::Screenshot)
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Screenshot)
        );
        assert!(preview.system_message.contains("<SCREENSHOT_CONTEXT>"));
        assert!(
            preview
                .system_message
                .contains("Do not describe the screenshot")
        );
        assert!(
            preview
                .system_message
                .contains("[SCREENSHOT_RESPONSE_EXAMPLES]")
        );

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(body.contains("\"type\":\"input_image\""));
        assert!(body.contains(screenshot_data_url));
    }

    #[tokio::test]
    async fn preview_prompt_ignores_screenshot_when_ocr_is_disabled() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"No screenshot\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" used.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot_data_url = "data:image/png;base64,SGVsbG8=";
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                screenshot: Some(voicewin_core::context::ImageArtifact {
                    data_url: screenshot_data_url.into(),
                }),
                screenshot_metadata: Some(voicewin_core::context::ScreenshotCaptureMetadata {
                    actual_scope: Some(voicewin_core::context::VisualCaptureScope::Display),
                    capture_elapsed_ms: Some(19),
                    fallback_reason: Some("foreground_window_not_implemented".into()),
                }),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-screenshot-disabled").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use the screenshot when it helps.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "read the screenshot".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "No screenshot used.");
        assert!(!preview.system_message.contains("<SCREENSHOT_CONTEXT>"));

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(!body.contains("\"type\":\"input_image\""));
        assert!(!body.contains(screenshot_data_url));
    }

    #[tokio::test]
    async fn preview_prompt_skips_screenshot_for_unsupported_api_and_surfaces_warning() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"choices":[{"message":{"content":"No screenshot used."}}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot_data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+i7wQAAAAASUVORK5CYII=";
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                screenshot: Some(voicewin_core::context::ImageArtifact {
                    data_url: screenshot_data_url.into(),
                }),
                screenshot_metadata: Some(voicewin_core::context::ScreenshotCaptureMetadata {
                    actual_scope: Some(voicewin_core::context::VisualCaptureScope::Display),
                    capture_elapsed_ms: Some(19),
                    fallback_reason: Some("foreground_window_not_implemented".into()),
                }),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-screenshot-legacy").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use the screenshot when it helps.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Screenshot,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "read the screenshot".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "No screenshot used.");
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Off)
        );
        assert!(!preview.system_message.contains("<SCREENSHOT_CONTEXT>"));
        assert_eq!(
            preview.warning.as_deref(),
            screenshot_context_warning("openai_compatible", "chat_completions").as_deref()
        );

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(!body.contains("\"type\":\"input_image\""));
        assert!(!body.contains(screenshot_data_url));
    }

    #[tokio::test]
    async fn preview_prompt_warns_when_visual_capture_returns_no_screenshot() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"VoiceWin\"}\n\n\
                 data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"VoiceWin\"}]}],\"usage\":{\"input_tokens\":256}}}\n\n\
                 data: [DONE]\n\n",
                "text/event-stream",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("linux"),
            ContextSnapshot::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-linux-visual").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use the screenshot when it helps.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Screenshot,
                    visual_capture_scope:
                        voicewin_core::context::VisualCaptureScope::ForegroundWindow,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "voice wen".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "VoiceWin");
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Off)
        );
        assert_eq!(
            preview.warning.as_deref(),
            voicewin_core::llm::visual_context_capture_unavailable_warning(
                VisualContextDispatch::Screenshot,
                voicewin_core::context::VisualCaptureScope::ForegroundWindow,
            )
            .as_deref()
        );

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(!body.contains("\"type\":\"input_image\""));
    }

    #[tokio::test]
    async fn preview_prompt_reports_ocr_runtime_metadata_for_text_only_api() {
        let server = MockServer::start().await;
        let ocr_sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"VOICE\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"WIN\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(ocr_sse, "text/event-stream"))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"choices":[{"message":{"content":"VoiceWin"}}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot_data_url = "data:image/png;base64,SGVsbG8=";
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                screenshot: Some(voicewin_core::context::ImageArtifact {
                    data_url: screenshot_data_url.into(),
                }),
                screenshot_metadata: Some(voicewin_core::context::ScreenshotCaptureMetadata {
                    actual_scope: Some(voicewin_core::context::VisualCaptureScope::Display),
                    capture_elapsed_ms: Some(19),
                    fallback_reason: Some("foreground_window_not_implemented".into()),
                }),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-preview-ocr").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Preview".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use OCR text when screenshots cannot be attached.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Ocr,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let preview = svc
            .preview_prompt(prompt, "read the screenshot".into(), None, false)
            .await
            .unwrap();

        assert_eq!(preview.final_output, "VoiceWin");
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.mode),
            Some(voicewin_core::context::VisualContextMode::Ocr)
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Ocr)
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.capture_actual_scope),
            Some(voicewin_core::context::VisualCaptureScope::Display)
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screenshot_capture_elapsed_ms),
            Some(19)
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.capture_fallback_reason.as_deref()),
            Some("foreground_window_not_implemented")
        );
        assert!(
            preview
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_elapsed_ms)
                .is_some()
        );
        assert_eq!(
            preview
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_text_chars),
            Some(8)
        );
        assert!(preview.system_message.contains("<SCREEN_OCR_TEXT>"));
        assert!(!preview.system_message.contains("<SCREENSHOT_CONTEXT>"));

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let ocr_body = std::str::from_utf8(&requests[0].body).unwrap();
        let main_body = std::str::from_utf8(&requests[1].body).unwrap();
        assert!(ocr_body.contains("\"type\":\"input_image\""));
        assert!(ocr_body.contains(screenshot_data_url));
        assert!(main_body.contains("<SCREEN_OCR_TEXT>"));
        assert!(!main_body.contains("\"type\":\"input_image\""));
    }

    #[tokio::test]
    async fn preview_prompt_with_app_snapshot_uses_supplied_app_and_context() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Replay\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" output.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("fallback.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-replay").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Replay".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Replay rewrite.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.invalid/v1".into(),
                llm_model: "default-model".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles {
                    use_clipboard: false,
                    use_selected_text: false,
                    use_window_context: false,
                    use_custom_vocabulary: false,
                    visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                    visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                },
            },
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "Mail Replay".into(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("mail.exe".into())],
                overrides: PowerModeOverrides {
                    llm_base_url: Some(server.uri()),
                    llm_model: Some("replay-model".into()),
                    context: Some(voicewin_core::context::ContextToggles {
                        use_clipboard: false,
                        use_selected_text: false,
                        use_window_context: true,
                        use_custom_vocabulary: false,
                        visual_context_mode: voicewin_core::context::VisualContextMode::Off,
                        visual_capture_scope: voicewin_core::context::VisualCaptureScope::Display,
                    }),
                    ..Default::default()
                },
            }],
            prompts: vec![prompt.clone()],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let app = AppIdentity::new()
            .with_process_name("mail.exe")
            .with_window_title("Inbox");
        let preview = svc
            .preview_prompt_with_app_snapshot(
                prompt,
                "replay transcript".into(),
                app,
                ContextSnapshot {
                    window_context: Some("Application: mail.exe\nActive Window: Inbox".into()),
                    ..Default::default()
                },
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(preview.app_process_name.as_deref(), Some("mail.exe"));
        assert_eq!(preview.app_window_title.as_deref(), Some("Inbox"));
        assert_eq!(preview.matched_profile_name.as_deref(), Some("Mail Replay"));
        assert_eq!(preview.model, "replay-model");
        assert_eq!(preview.final_output, "Replay output.");
        assert_eq!(preview.warning, None);
        assert!(preview.system_message.contains("<CURRENT_WINDOW_CONTEXT>"));
        assert!(preview.system_message.contains("Application: mail.exe"));
    }

    #[tokio::test]
    async fn probe_llm_provider_returns_latency_and_output() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"VoiceWin provider\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" probe ok.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-probe").unwrap();

        let response = svc
            .probe_llm_provider(
                "openai_compatible",
                "responses_sse",
                &server.uri(),
                "gpt-5.4",
                Some("low"),
                ProviderProbeKind::Smoke,
            )
            .await
            .unwrap();

        assert_eq!(response.probe_kind, ProviderProbeKind::Smoke);
        assert_eq!(response.provider_kind, "openai_compatible");
        assert_eq!(response.api_kind, "responses_sse");
        assert_eq!(response.model, "gpt-5.4");
        assert!(response.elapsed_ms >= 1);
        assert!(response.first_token_ms.is_some());
        assert_eq!(response.expected_output, PROVIDER_PROBE_SMOKE_OUTPUT);
        assert_eq!(response.final_output, "VoiceWin provider probe ok.");
        assert_eq!(response.warning, None);
    }

    #[tokio::test]
    async fn probe_llm_provider_screenshot_probe_attaches_image_and_surfaces_warning() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"The screenshot contains a single visible word in the center:\\n\\n**VoiceWin**\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        svc.set_openai_api_key("sk-probe").unwrap();

        let response = svc
            .probe_llm_provider(
                "openai_compatible",
                "responses_sse",
                &server.uri(),
                "gpt-5.4",
                None,
                ProviderProbeKind::ScreenshotProductName,
            )
            .await
            .unwrap();

        assert_eq!(
            response.probe_kind,
            ProviderProbeKind::ScreenshotProductName
        );
        assert_eq!(response.expected_output, PROVIDER_PROBE_SCREENSHOT_OUTPUT);
        assert_eq!(response.final_output, "VoiceWin");
        assert_eq!(
            response.warning.as_deref(),
            Some(
                "LLM output looked conversational; VoiceWin stripped assistant framing from the model output."
            )
        );

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(body.contains("\"type\":\"input_image\""));
        assert!(body.contains("\"image_url\":\"data:image/png;base64,"));
        assert!(body.contains("voice wen"));
    }

    #[tokio::test]
    async fn insertion_failure_merges_warning_and_persists_recovered_text_to_history() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = voicewin_platform::test::TestContextProvider::new(
            voicewin_core::types::AppIdentity::new().with_process_name("slack.exe"),
            Default::default(),
        )
        .boxed();

        let svc = AppService::new(config_path.clone(), ctx, Arc::new(FailingInserter));

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec!["rewrite".into()],
            }],
            llm_api_key_present: false,
        };

        svc.save_config(&cfg).unwrap();

        let res = svc
            .run_session(
                RunSessionRequest {
                    transcript: "hello world".into(),
                    warning: Some("Realtime warning".into()),
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![0.0; 160],
                },
            )
            .await
            .unwrap();

        assert_eq!(res.stage, "failed");
        assert_eq!(res.final_text.as_deref(), Some("hello world"));
        assert_eq!(res.warning.as_deref(), Some("Realtime warning"));
        assert_eq!(res.error.as_deref(), Some("simulated insert failure"));

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "hello world");
        assert_eq!(history[0].stage, "failed");
        assert_eq!(history[0].warning.as_deref(), Some("Realtime warning"));
        assert_eq!(
            history[0].error.as_deref(),
            Some("simulated insert failure")
        );
        assert_eq!(history[0].app_process_name.as_deref(), Some("slack.exe"));
    }

    #[tokio::test]
    async fn successful_session_surfaces_post_process_warning_and_persists_history() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"I'll send this as a clear follow-up:\\n\\n\\\"Please ship the VoiceWin update later this week.\\\"\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let inserter = Arc::new(MemoryInserter::default());
        let svc = AppService::new(config_path.clone(), ctx, inserter.clone());

        svc.set_openai_api_key("sk-run-warning").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Email".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Rewrite as a clean email sentence.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![prompt],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let response = svc
            .run_session(
                RunSessionRequest {
                    transcript: "please ship the voice win update later this week".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        assert_eq!(
            response.final_text.as_deref(),
            Some("Please ship the VoiceWin update later this week.")
        );
        assert_eq!(
            response.warning.as_deref(),
            Some(
                "LLM output looked conversational; VoiceWin stripped assistant framing from the model output."
            )
        );
        assert_eq!(response.error, None);

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(
            inserted[0],
            (
                "Please ship the VoiceWin update later this week.".into(),
                InsertMode::Paste
            )
        );
        drop(inserted);

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].warning.as_deref(),
            Some(
                "LLM output looked conversational; VoiceWin stripped assistant framing from the model output."
            )
        );
        assert_eq!(history[0].error, None);
        assert_eq!(
            history[0].text,
            "Please ship the VoiceWin update later this week."
        );
    }

    #[tokio::test]
    async fn successful_session_strips_instruction_echo_wrapper_and_persists_warning() {
        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Turn this into a polished sentence: Hello, VoiceWin world.\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            Default::default(),
        )
        .boxed();
        let inserter = Arc::new(MemoryInserter::default());
        let svc = AppService::new(config_path.clone(), ctx, inserter.clone());

        svc.set_openai_api_key("sk-run-instruction-echo").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Cleanup".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Fix grammar.".into(),
            trigger_words: vec![],
        };
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(prompt.id.clone()),
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![prompt],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let response = svc
            .run_session(
                RunSessionRequest {
                    transcript: "turn this into a polished sentence: hello voicewin world".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        assert_eq!(
            response.final_text.as_deref(),
            Some("Hello, VoiceWin world.")
        );
        assert_eq!(
            response.warning.as_deref(),
            Some(
                "LLM output echoed transcript instruction framing; VoiceWin stripped the instruction wrapper from the model output."
            )
        );

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(
            inserted[0],
            ("Hello, VoiceWin world.".into(), InsertMode::Paste)
        );
        drop(inserted);

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "Hello, VoiceWin world.");
        assert_eq!(
            history[0].warning.as_deref(),
            Some(
                "LLM output echoed transcript instruction framing; VoiceWin stripped the instruction wrapper from the model output."
            )
        );
    }

    #[tokio::test]
    async fn runtime_session_skips_screenshot_for_unsupported_api_and_persists_warning() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"choices":[{"message":{"content":"VoiceWin restored."}}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot::default(),
        ));
        let screenshot_data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+i7wQAAAAASUVORK5CYII=";
        ctx.set_captured_screenshot(Some(voicewin_core::context::ImageArtifact {
            data_url: screenshot_data_url.into(),
        }));
        let inserter = Arc::new(MemoryInserter::default());
        let svc = AppService::new(config_path.clone(), ctx.clone(), inserter.clone());

        svc.set_openai_api_key("sk-run-legacy").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Cleanup".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Rewrite cleanly.".into(),
            trigger_words: vec![],
        };
        let mut defaults = GlobalDefaults {
            enable_enhancement: true,
            prompt_id: Some(prompt.id.clone()),
            insert_mode: InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "./missing.bin".into(),
            language: "en".into(),
            llm_provider_kind: "openai_compatible".into(),
            llm_base_url: server.uri(),
            llm_model: "gpt-5.4".into(),
            llm_api_kind: "chat_completions".into(),
            llm_preflight_mode: "off".into(),
            llm_preflight_delay_ms: 1_500,
            screenshot_max_edge_px: 1_280,
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: voicewin_core::context::ContextToggles::default(),
        };
        defaults.context.use_clipboard = false;
        defaults.context.use_selected_text = false;
        defaults.context.use_window_context = false;
        defaults.context.use_custom_vocabulary = false;
        defaults.context.visual_context_mode =
            voicewin_core::context::VisualContextMode::Screenshot;
        let cfg = AppConfig {
            defaults,
            profiles: vec![],
            prompts: vec![prompt],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let response = svc
            .run_session(
                RunSessionRequest {
                    transcript: "voice wen restored".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        assert_eq!(response.final_text.as_deref(), Some("VoiceWin restored."));
        assert_eq!(
            response.warning.as_deref(),
            screenshot_context_warning("openai_compatible", "chat_completions").as_deref()
        );
        assert_eq!(ctx.screenshot_capture_count(), 0);

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(
            inserted[0],
            ("VoiceWin restored.".into(), InsertMode::Paste)
        );
        drop(inserted);

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body = std::str::from_utf8(&requests[0].body).unwrap();
        assert!(!body.contains("\"type\":\"input_image\""));
        assert!(!body.contains(screenshot_data_url));

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "VoiceWin restored.");
        assert_eq!(
            history[0]
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.mode),
            Some(voicewin_core::context::VisualContextMode::Screenshot)
        );
        assert_eq!(
            history[0]
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Off)
        );
        assert_eq!(
            history[0].warning.as_deref(),
            screenshot_context_warning("openai_compatible", "chat_completions").as_deref()
        );
        assert_eq!(history[0].error, None);
    }

    #[tokio::test]
    async fn explicit_run_config_overrides_persisted_insert_mode_without_mutating_disk_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = voicewin_platform::test::TestContextProvider::new(
            voicewin_core::types::AppIdentity::new().with_process_name("notepad.exe"),
            Default::default(),
        )
        .boxed();
        let inserter = Arc::new(MemoryInserter::default());

        let svc = AppService::new(config_path.clone(), ctx, inserter.clone());

        let persisted = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::PasteAndEnter,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&persisted).unwrap();

        let mut override_cfg = persisted.clone();
        override_cfg.defaults.insert_mode = InsertMode::Paste;

        let response = svc
            .run_session_with_hook_using_config(
                override_cfg,
                RunSessionRequest {
                    transcript: "hello world".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
                |_stage| async {},
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0], ("hello world".into(), InsertMode::Paste));
        drop(inserted);

        let loaded = svc.load_config().unwrap();
        assert_eq!(loaded.defaults.insert_mode, InsertMode::PasteAndEnter);
    }

    #[tokio::test]
    async fn transcript_override_does_not_touch_local_model_path_during_explicit_run() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = voicewin_platform::test::TestContextProvider::new(
            voicewin_core::types::AppIdentity::new().with_process_name("textedit"),
            Default::default(),
        )
        .boxed();
        let inserter = Arc::new(MemoryInserter::default());

        let svc = AppService::new(config_path, ctx, inserter.clone());
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "/definitely/does/not/exist.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };

        let response = svc
            .run_session_with_hook_using_config(
                cfg,
                RunSessionRequest {
                    transcript: "runtime smoke transcript".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
                |_stage| async {},
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        assert_eq!(
            response.final_text.as_deref(),
            Some("runtime smoke transcript")
        );

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(
            inserted[0],
            ("runtime smoke transcript".into(), InsertMode::Paste)
        );
    }

    #[tokio::test]
    async fn prepared_session_context_freezes_start_time_app_context_and_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("slack.exe"),
            ContextSnapshot {
                clipboard: Some("start-clipboard".into()),
                ..Default::default()
            },
        ));
        let inserter = Arc::new(MemoryInserter::default());
        let svc = AppService::new(config_path.clone(), ctx.clone(), inserter.clone());

        let start_profile_id = ProfileId::new();
        let start_cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "/definitely/does/not/exist.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![PowerModeProfile {
                id: start_profile_id.clone(),
                name: "Slack".into(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("slack.exe".into())],
                overrides: PowerModeOverrides {
                    insert_mode: Some(InsertMode::PasteAndEnter),
                    ..Default::default()
                },
            }],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&start_cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(
            prepared.effective_config.matched_profile_id,
            Some(start_profile_id)
        );
        assert_eq!(
            prepared.effective_config.insert_mode,
            InsertMode::PasteAndEnter
        );
        assert_eq!(
            prepared
                .app
                .process_name
                .as_ref()
                .map(|name| name.0.as_str()),
            Some("slack.exe")
        );
        assert_eq!(
            prepared.context.clipboard.as_deref(),
            Some("start-clipboard")
        );

        ctx.set(
            AppIdentity::new().with_process_name("notepad.exe"),
            ContextSnapshot {
                clipboard: Some("stop-clipboard".into()),
                ..Default::default()
            },
        );

        let mut stop_cfg = start_cfg.clone();
        stop_cfg.defaults.insert_mode = InsertMode::ShiftInsert;
        stop_cfg.profiles.clear();
        svc.save_config(&stop_cfg).unwrap();

        let response = svc
            .run_session_with_prepared_with_hook(
                prepared,
                RunSessionRequest {
                    transcript: "prepared transcript".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
                |_stage| async {},
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(
            inserted[0],
            ("prepared transcript".into(), InsertMode::PasteAndEnter)
        );
        drop(inserted);

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].app_process_name.as_deref(), Some("slack.exe"));
        assert_eq!(history[0].matched_profile_name.as_deref(), Some("Slack"));
        assert_eq!(history[0].text, "prepared transcript");
    }

    #[tokio::test]
    async fn prepare_session_context_attaches_captured_screenshot_when_ocr_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot = voicewin_core::context::ImageArtifact {
            data_url: "data:image/png;base64,SGVsbG8=".into(),
        };

        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot::default(),
        ));
        ctx.set_captured_screenshot(Some(screenshot.clone()));
        ctx.set_captured_screenshot_metadata(Some(
            voicewin_core::context::ScreenshotCaptureMetadata {
                actual_scope: Some(voicewin_core::context::VisualCaptureScope::ForegroundWindow),
                capture_elapsed_ms: Some(23),
                fallback_reason: None,
            },
        ));
        let svc = AppService::new(
            config_path,
            ctx.clone(),
            Arc::new(MemoryInserter::default()),
        );

        let mut defaults = GlobalDefaults {
            enable_enhancement: false,
            prompt_id: None,
            insert_mode: InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "./missing.bin".into(),
            language: "en".into(),
            llm_provider_kind: "openai_compatible".into(),
            llm_base_url: "https://example.com/v1".into(),
            llm_model: "gpt-5.4".into(),
            llm_api_kind: "responses_sse".into(),
            llm_preflight_mode: "off".into(),
            llm_preflight_delay_ms: 1_500,
            screenshot_max_edge_px: 640,
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: voicewin_core::context::ContextToggles::default(),
        };
        defaults.context.visual_context_mode =
            voicewin_core::context::VisualContextMode::Screenshot;
        let cfg = AppConfig {
            defaults,
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(prepared.context.screenshot, Some(screenshot));
        assert_eq!(
            prepared
                .context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.actual_scope),
            Some(voicewin_core::context::VisualCaptureScope::ForegroundWindow)
        );
        assert_eq!(
            prepared
                .context
                .screenshot_metadata
                .as_ref()
                .and_then(|metadata| metadata.capture_elapsed_ms),
            Some(23)
        );
        assert_eq!(ctx.screenshot_capture_count(), 1);
        assert_eq!(
            ctx.last_screenshot_options().as_slice(),
            &[voicewin_engine::traits::ScreenshotCaptureOptions {
                max_edge_px: 640,
                scope: voicewin_core::context::VisualCaptureScope::Display
            }]
        );
    }

    #[tokio::test]
    async fn prepared_session_context_precomputes_ocr_for_text_only_api_and_reuses_it_at_stop() {
        let server = MockServer::start().await;
        let ocr_sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"VOICE\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"WIN\"}\n\n",
            "data: [DONE]\n\n"
        );

        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(ocr_sse, "text/event-stream"))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"choices":[{"message":{"content":"VoiceWin"}}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let screenshot = voicewin_core::context::ImageArtifact {
            data_url: "data:image/png;base64,SGVsbG8=".into(),
        };

        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot::default(),
        ));
        ctx.set_captured_screenshot(Some(screenshot));
        let inserter = Arc::new(MemoryInserter::default());
        let svc = AppService::new(config_path.clone(), ctx.clone(), inserter.clone());

        svc.set_openai_api_key("sk-prepared-ocr").unwrap();

        let prompt = PromptTemplate {
            id: PromptId::new(),
            title: "Cleanup".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Use OCR text when helpful.".into(),
            trigger_words: vec![],
        };
        let mut defaults = GlobalDefaults {
            enable_enhancement: true,
            prompt_id: Some(prompt.id.clone()),
            insert_mode: InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "./missing.bin".into(),
            language: "en".into(),
            llm_provider_kind: "openai_compatible".into(),
            llm_base_url: server.uri(),
            llm_model: "gpt-5.4".into(),
            llm_api_kind: "chat_completions".into(),
            llm_preflight_mode: "off".into(),
            llm_preflight_delay_ms: 1_500,
            screenshot_max_edge_px: 640,
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: voicewin_core::context::ContextToggles::default(),
        };
        defaults.context.visual_context_mode = voicewin_core::context::VisualContextMode::Ocr;
        let cfg = AppConfig {
            defaults,
            profiles: vec![],
            prompts: vec![prompt],
            llm_api_key_present: true,
        };
        svc.save_config(&cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(
            prepared
                .context
                .precomputed_screen_ocr
                .as_ref()
                .map(|ocr| ocr.text.as_str()),
            Some("VOICEWIN")
        );
        assert!(
            prepared
                .context
                .precomputed_screen_ocr
                .as_ref()
                .is_some_and(|ocr| ocr.elapsed_ms > 0)
        );
        assert_eq!(ctx.screenshot_capture_count(), 1);

        let response = svc
            .run_session_with_prepared_with_hook(
                prepared,
                RunSessionRequest {
                    transcript: "voice wen".into(),
                    warning: None,
                },
                AudioInput {
                    sample_rate_hz: 16_000,
                    samples: vec![],
                },
                |_stage| async {},
            )
            .await
            .unwrap();

        assert_eq!(response.stage, "done");
        assert_eq!(response.final_text.as_deref(), Some("VoiceWin"));
        assert_eq!(response.warning, None);

        let inserted = inserter.inserted.lock().unwrap();
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0], ("VoiceWin".into(), InsertMode::Paste));
        drop(inserted);

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path() == "/responses")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path() == "/chat/completions")
                .count(),
            1
        );

        let history = HistoryStore::at_path(dir.path().join("history.json"))
            .load()
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "VoiceWin");
        assert_eq!(
            history[0]
                .visual_context_runtime
                .as_ref()
                .map(|runtime| runtime.dispatch),
            Some(VisualContextDispatch::Ocr)
        );
        assert!(
            history[0]
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_elapsed_ms)
                .is_some_and(|value| value > 0)
        );
        assert_eq!(
            history[0]
                .visual_context_runtime
                .as_ref()
                .and_then(|runtime| runtime.screen_ocr_text_chars),
            Some(8)
        );
    }

    #[tokio::test]
    async fn prepare_session_context_skips_captured_screenshot_when_ocr_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot::default(),
        ));
        ctx.set_captured_screenshot(Some(voicewin_core::context::ImageArtifact {
            data_url: "data:image/png;base64,SGVsbG8=".into(),
        }));
        let svc = AppService::new(
            config_path,
            ctx.clone(),
            Arc::new(MemoryInserter::default()),
        );

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(prepared.context.screenshot, None);
        assert_eq!(ctx.screenshot_capture_count(), 0);
        assert!(ctx.last_screenshot_options().is_empty());
    }

    #[tokio::test]
    async fn prepare_session_context_skips_captured_screenshot_when_api_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let ctx = Arc::new(MutableContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot::default(),
        ));
        ctx.set_captured_screenshot(Some(voicewin_core::context::ImageArtifact {
            data_url:
                "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+i7wQAAAAASUVORK5CYII="
                    .into(),
        }));
        let svc = AppService::new(
            config_path,
            ctx.clone(),
            Arc::new(MemoryInserter::default()),
        );

        let mut defaults = GlobalDefaults {
            enable_enhancement: false,
            prompt_id: None,
            insert_mode: InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "./missing.bin".into(),
            language: "en".into(),
            llm_provider_kind: "openai_compatible".into(),
            llm_base_url: "https://example.com/v1".into(),
            llm_model: "gpt-5.4".into(),
            llm_api_kind: "chat_completions".into(),
            llm_preflight_mode: "off".into(),
            llm_preflight_delay_ms: 1_500,
            screenshot_max_edge_px: 1_280,
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: voicewin_core::context::ContextToggles::default(),
        };
        defaults.context.visual_context_mode =
            voicewin_core::context::VisualContextMode::Screenshot;
        let cfg = AppConfig {
            defaults,
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(prepared.context.screenshot, None);
        assert_eq!(ctx.screenshot_capture_count(), 0);
        assert!(ctx.last_screenshot_options().is_empty());
    }

    #[tokio::test]
    async fn prepare_session_context_merges_custom_vocabulary_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(
            dir.path().join("custom_vocabulary.txt"),
            " VoiceWin \nScribe\nVoiceWin\n",
        )
        .unwrap();

        let ctx = voicewin_platform::test::TestContextProvider::new(
            AppIdentity::new().with_process_name("mail.exe"),
            ContextSnapshot {
                custom_vocabulary: Some("Gemini\nVoiceWin".into()),
                ..Default::default()
            },
        )
        .boxed();
        let svc = AppService::new(
            config_path,
            ctx,
            Arc::new(voicewin_platform::test::StdoutInserter),
        );

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(
            prepared.context.custom_vocabulary.as_deref(),
            Some("Gemini\nVoiceWin\nScribe")
        );
    }

    #[tokio::test]
    async fn prepare_session_context_starts_preflight_only_after_configured_delay() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"data\":[]}"))
            .mount(&server)
            .await;

        let svc = AppService::new(
            config_path,
            voicewin_platform::test::TestContextProvider::new(
                AppIdentity::new().with_process_name("mail.exe"),
                ContextSnapshot::default(),
            )
            .boxed(),
            Arc::new(MemoryInserter::default()),
        );

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 75,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec![],
            }],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();
        svc.set_openai_api_key("sk-test").unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 0);

        tokio::time::sleep(Duration::from_millis(125)).await;

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method.as_str(), "GET");
        assert_eq!(requests[0].url.path(), "/models");

        drop(prepared);
    }

    #[tokio::test]
    async fn prepared_session_context_can_abort_delayed_preflight_before_it_fires() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{\"data\":[]}"))
            .mount(&server)
            .await;

        let svc = AppService::new(
            config_path,
            voicewin_platform::test::TestContextProvider::new(
                AppIdentity::new().with_process_name("mail.exe"),
                ContextSnapshot::default(),
            )
            .boxed(),
            Arc::new(MemoryInserter::default()),
        );

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "./missing.bin".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: server.uri(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 250,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec![],
            }],
            llm_api_key_present: false,
        };
        svc.save_config(&cfg).unwrap();
        svc.set_openai_api_key("sk-test").unwrap();

        let prepared = svc.prepare_session_context().await.unwrap();
        prepared.abort_preflight();

        tokio::time::sleep(Duration::from_millis(325)).await;

        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[test]
    fn normalize_custom_vocabulary_text_dedupes_and_trims() {
        let normalized =
            normalize_custom_vocabulary_text(" VoiceWin \n\nScribe\nVoiceWin\n  Scribe  \n");
        assert_eq!(normalized.as_deref(), Some("VoiceWin\nScribe"));
    }

    #[test]
    fn normalize_custom_vocabulary_text_respects_max_bytes() {
        let first = "a".repeat(CUSTOM_VOCABULARY_MAX_BYTES - 1);
        let normalized = normalize_custom_vocabulary_text(&format!("{first}\nzz"));
        assert_eq!(normalized.as_deref(), Some(first.as_str()));
    }

    #[test]
    fn llm_preflight_requires_http_connect_mode_and_prompt_capability() {
        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: false,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "mock".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-5.4".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: None,
                microphone_device: None,
                microphone_device_id: None,
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec!["rewrite".into()],
            }],
            llm_api_key_present: false,
        };

        let effective = resolve_effective_config(
            &cfg.defaults,
            &cfg.profiles,
            &AppIdentity::new().with_process_name("slack.exe"),
            &EphemeralOverrides::default(),
        );

        assert!(should_preflight_llm(&cfg, &effective, "sk-test"));

        let mut disabled = effective.clone();
        disabled.llm_preflight_mode = "off".into();
        assert!(!should_preflight_llm(&cfg, &disabled, "sk-test"));

        assert!(!should_preflight_llm(&cfg, &effective, ""));

        let mut no_prompts = cfg.clone();
        no_prompts.prompts.clear();
        assert!(!should_preflight_llm(&no_prompts, &effective, "sk-test"));
    }
}

use std::sync::Arc;
#[cfg(any(windows, target_os = "macos"))]
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool as GateAtomicBool, Ordering as GateOrdering};
#[cfg(any(windows, target_os = "macos"))]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use voicewin_appcore::service::AppService;

#[cfg(any(windows, target_os = "macos"))]
use voicewin_runtime::secrets::{SecretKey, get_secret};

#[cfg(any(windows, target_os = "macos"))]
use voicewin_providers::elevenlabs_realtime::{
    ElevenLabsRealtimeConfig, ElevenLabsRealtimeHandle, RealtimeEvent, spawn_realtime_session,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStage {
    Idle,
    Recording,
    // Realtime-only: we already streamed audio during recording, so after stop we may be
    // finalizing the transcript (and/or running post-processing) rather than doing batch STT.
    Finalizing,
    Transcribing,

    // These are emitted via the engine stage hook, but depending on config/user settings
    // they may be skipped (e.g. enhancement disabled).
    Enhancing,
    Inserting,

    Success,
    Error,
    Cancelled,
}

// `cargo check` on Linux doesn't compile the Windows/macOS recording path; keep warnings down.
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(dead_code)]
const _STAGE_KEEPALIVE_FINALIZING: SessionStage = SessionStage::Finalizing;
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(dead_code)]
const _STAGE_KEEPALIVE_TRANSCRIBING: SessionStage = SessionStage::Transcribing;
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(dead_code)]
const _STAGE_KEEPALIVE_ENHANCING: SessionStage = SessionStage::Enhancing;
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(dead_code)]
const _STAGE_KEEPALIVE_INSERTING: SessionStage = SessionStage::Inserting;
#[cfg(not(any(windows, target_os = "macos")))]
#[allow(dead_code)]
const _STAGE_KEEPALIVE_SUCCESS: SessionStage = SessionStage::Success;

impl Default for SessionStage {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatusPayload {
    pub stage: SessionStage,
    pub stage_label: String,
    pub is_recording: bool,
    pub elapsed_ms: Option<u64>,
    pub error: Option<String>,
    // Reserved for future use (e.g. transcript preview in the main window).
    pub last_text_preview: Option<String>,
    pub last_text_available: bool,
}

#[cfg(any(windows, target_os = "macos", test))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct MicLevelPayload {
    pub rms: f32,
    pub peak: f32,
}

#[cfg(any(windows, target_os = "macos"))]
enum RealtimeUploaderCmd {
    // Drain any queued frames and report whether all audio was sent successfully.
    Drain {
        respond_to: tokio::sync::oneshot::Sender<bool>,
    },
}

#[cfg(any(windows, target_os = "macos"))]
struct RealtimeSttState {
    handle: ElevenLabsRealtimeHandle,
    uploader_cmd_tx: tokio::sync::mpsc::Sender<RealtimeUploaderCmd>,
    uploader_task: tauri::async_runtime::JoinHandle<()>,
    receiver_task: tauri::async_runtime::JoinHandle<()>,
    streaming_enabled: Arc<AtomicBool>,
    dropped_chunks: Arc<AtomicU64>,

    // Lightweight diagnostics for stop-time catch-up.
    captured_frames: Arc<AtomicU64>,
    sent_frames: Arc<AtomicU64>,

    // Used to flush the final partial frame on stop.
    audio_tx: tokio::sync::mpsc::Sender<Vec<f32>>,
    chunker: Arc<StdMutex<RealtimeFrameChunker>>,

    // Best-effort diagnostics/warnings to surface on stop (and persist to History).
    last_error: Arc<StdMutex<Option<String>>>,
    last_warning: Arc<StdMutex<Option<String>>>,
}

#[cfg(any(windows, target_os = "macos"))]
const REALTIME_FRAME_MS_DEFAULT: u64 = 50;

#[cfg(any(windows, target_os = "macos"))]
const REALTIME_BUFFER_MS_DEFAULT: u64 = 5_000;

#[derive(Default)]
struct Inner {
    stage: SessionStage,
    recording_started_at: Option<Instant>,
    recording_elapsed_ms: Option<u64>,
    last_text: Option<String>,
    status_message: Option<String>,
    status_message_expires_at: Option<Instant>,
    session_id: u64,

    // Set by the overlay webview calling `overlay_ready`.
    // We use it to make status delivery more reliable (re-emit after listeners attach).
    overlay_ready: bool,

    // When we stop recording we run the session pipeline (transcribe/enhance/insert)
    // in a background task so the UI stays responsive and we can cancel it.
    processing_task: Option<tauri::async_runtime::JoinHandle<()>>,

    #[cfg(any(windows, target_os = "macos"))]
    realtime_stt: Option<RealtimeSttState>,
}

struct TransitionGuard {
    gate: Arc<GateAtomicBool>,
}

#[cfg(test)]
#[derive(Default)]
struct TransitionPauseHook {
    armed: GateAtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl Drop for TransitionGuard {
    fn drop(&mut self) {
        self.gate.store(false, GateOrdering::Release);
    }
}

#[derive(Clone, Default)]
pub struct SessionController {
    #[allow(dead_code)]
    inner: Arc<Mutex<Inner>>,
    transition_gate: Arc<GateAtomicBool>,
    #[cfg(test)]
    transition_pause: Arc<TransitionPauseHook>,
}

impl SessionController {
    const MAX_RECORDING_DURATION: Duration = Duration::from_secs(120);
    const BUSY_TOAST_TTL: Duration = Duration::from_secs(1);
    // Design-draft: Success state must remain visible for 1500ms before exit.
    const OVERLAY_HIDE_DELAY: Duration = Duration::from_millis(1500);

    pub fn new() -> Self {
        Self::default()
    }

    fn try_acquire_transition(&self) -> Option<TransitionGuard> {
        if self.transition_gate.swap(true, GateOrdering::AcqRel) {
            return None;
        }

        Some(TransitionGuard {
            gate: self.transition_gate.clone(),
        })
    }

    #[cfg(test)]
    fn arm_transition_pause(&self) {
        self.transition_pause.armed.store(true, GateOrdering::Release);
    }

    #[cfg(test)]
    async fn wait_for_transition_pause(&self) {
        self.transition_pause.entered.notified().await;
    }

    #[cfg(test)]
    fn release_transition_pause(&self) {
        self.transition_pause.release.notify_one();
    }

    #[cfg(test)]
    async fn maybe_pause_after_transition_acquired(&self) {
        if self.transition_pause.armed.swap(false, GateOrdering::AcqRel) {
            self.transition_pause.entered.notify_one();
            self.transition_pause.release.notified().await;
        }
    }

    #[cfg(not(test))]
    async fn maybe_pause_after_transition_acquired(&self) {}

    fn prune_status_message(inner: &mut Inner) {
        if let Some(expires_at) = inner.status_message_expires_at {
            if Instant::now() >= expires_at {
                inner.status_message = None;
                inner.status_message_expires_at = None;
            }
        }
    }

    #[allow(dead_code)]
    pub async fn get_status(&self) -> SessionStatusPayload {
        let mut inner = self.inner.lock().await;
        Self::prune_status_message(&mut inner);

        let elapsed_ms = if inner.stage == SessionStage::Recording {
            inner
                .recording_started_at
                .map(|t| t.elapsed())
                .map(|d| d.as_millis() as u64)
        } else {
            inner.recording_elapsed_ms
        };

        let last_text_preview = inner.last_text.as_ref().map(|t| preview_text(t));

        SessionStatusPayload {
            stage: inner.stage,
            stage_label: stage_label(inner.stage).into(),
            is_recording: inner.stage == SessionStage::Recording,
            elapsed_ms,
            error: inner.status_message.clone(),
            last_text_preview,
            last_text_available: inner
                .last_text
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false),
        }
    }

    pub async fn mark_overlay_ready<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
    ) {
        {
            let mut inner = self.inner.lock().await;
            if !inner.overlay_ready {
                inner.overlay_ready = true;
                log::info!("overlay_ready received");
            }
        }

        // Immediately push the current status so the overlay can't miss the first stage.
        self.emit_status(app).await;
    }

    pub async fn emit_status<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
    ) {
        let payload = {
            let mut inner = self.inner.lock().await;
            Self::prune_status_message(&mut inner);

            let elapsed_ms = if inner.stage == SessionStage::Recording {
                inner
                    .recording_started_at
                    .map(|t| t.elapsed())
                    .map(|d| d.as_millis() as u64)
            } else {
                inner.recording_elapsed_ms
            };

            let last_text_preview = inner.last_text.as_ref().map(|t| preview_text(t));

            SessionStatusPayload {
                stage: inner.stage,
                stage_label: stage_label(inner.stage).into(),
                is_recording: inner.stage == SessionStage::Recording,
                elapsed_ms,
                error: inner.status_message.clone(),
                last_text_preview,
                last_text_available: inner
                    .last_text
                    .as_ref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false),
            }
        };

        // Best-effort: emit directly to the overlay window for reliability.
        if let Some(w) = app.get_webview_window("recording_overlay") {
            if let Err(e) = w.emit(crate::EVENT_SESSION_STATUS, payload.clone()) {
                log::warn!("emit session status to overlay failed: {e}");
            }
        }

        if let Err(e) = app.emit(crate::EVENT_SESSION_STATUS, payload) {
            log::warn!("emit session status failed: {e}");
        }
    }

    #[cfg(any(windows, target_os = "macos", test))]
    pub async fn emit_mic_level<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        rms: f32,
        peak: f32,
    ) {
        let payload = MicLevelPayload {
            rms: rms.clamp(0.0, 1.0),
            peak: peak.clamp(0.0, 1.0),
        };

        if let Some(w) = app.get_webview_window("recording_overlay") {
            if let Err(e) = w.emit(crate::EVENT_MIC_LEVEL, payload.clone()) {
                log::warn!("emit mic level to overlay failed: {e}");
            }
        }

        if let Err(e) = app.emit(crate::EVENT_MIC_LEVEL, payload) {
            log::warn!("emit mic level failed: {e}");
        }
    }

    pub async fn set_stage<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        stage: SessionStage,
    ) {
        {
            let mut inner = self.inner.lock().await;
            apply_stage_transition(&mut inner, stage);
        }
        self.emit_status(app).await;
    }

    #[allow(dead_code)]
    pub async fn set_last_text(&self, text: Option<String>) {
        let mut inner = self.inner.lock().await;
        inner.last_text = text;
    }

    fn show_overlay<R: tauri::Runtime + 'static>(app: &tauri::AppHandle<R>) {
        if let Some(w) = app.get_webview_window("recording_overlay") {
            let _ = w.show();
        }
    }

    async fn show_overlay_and_sync<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
    ) {
        // On some platforms a hidden webview may miss events; showing first and
        // then emitting status makes the overlay self-healing.
        Self::show_overlay(app);
        self.emit_status(app).await;
    }

    async fn hide_overlay_if_session_matches<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        session_id: u64,
        delay: Duration,
    ) {
        // Delay a bit so the user can see the completed stage/message.
        tokio::time::sleep(delay).await;

        let should_hide = {
            let inner = self.inner.lock().await;
            inner.session_id == session_id
                && matches!(inner.stage, SessionStage::Success | SessionStage::Cancelled)
        };

        if should_hide {
            if let Some(w) = app.get_webview_window("recording_overlay") {
                let _ = w.hide();
            }
        }
    }

    async fn set_status_message<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        message: String,
        ttl: Duration,
    ) {
        {
            let mut inner = self.inner.lock().await;
            inner.status_message = Some(message);
            inner.status_message_expires_at = Some(Instant::now() + ttl);
        }
        self.emit_status(app).await;
    }

    #[allow(dead_code)]
    async fn mark_error<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        error: String,
    ) {
        log::error!("session error: {error}");
        self.set_stage(app, SessionStage::Error).await;
        self.set_status_message(app, error, Duration::from_secs(6))
            .await;

        // Always surface errors in the HUD.
        self.show_overlay_and_sync(app).await;
    }

    async fn apply_session_result<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        session_id: u64,
        decision: SessionResultDecision,
    ) -> bool {
        let outcome = {
            let mut inner = self.inner.lock().await;
            apply_session_result_to_inner(&mut inner, session_id, decision)
        };

        if let Some(message) = outcome.log_message.as_ref() {
            log::error!("{message}");
        }

        match outcome.side_effect {
            SessionResultSideEffect::Ignore => false,
            SessionResultSideEffect::ShowOverlay => {
                if self.inner.lock().await.session_id == session_id {
                    self.show_overlay_and_sync(app).await;
                }
                true
            }
            SessionResultSideEffect::HideOverlayAfter(delay) => {
                self.emit_status(app).await;
                let controller = self.clone();
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    controller
                        .hide_overlay_if_session_matches(&app_handle, session_id, delay)
                        .await;
                });
                true
            }
        }
    }

    pub async fn cancel_recording<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        svc: AppService,
    ) -> ToggleResult {
        let _transition_guard = match self.try_acquire_transition() {
            Some(g) => g,
            None => {
                self.set_status_message(app, "busy".into(), Self::BUSY_TOAST_TTL)
                    .await;
                return ToggleResult {
                    stage: "busy".into(),
                    final_text: None,
                    error: Some("busy".into()),
                    is_recording: self.inner.lock().await.stage == SessionStage::Recording,
                };
            }
        };

        self.maybe_pause_after_transition_acquired().await;

        let (stage, current_session_id, has_processing_task) = {
            let inner = self.inner.lock().await;
            (inner.stage, inner.session_id, inner.processing_task.is_some())
        };

        match resolve_cancel_decision(stage, current_session_id, has_processing_task) {
            CancelDecision::Cancel(decision) if decision.should_cancel_recorder => {
                #[cfg(any(windows, target_os = "macos"))]
                {
                    // Stop any realtime streaming immediately.
                    let rt = {
                        let mut inner = self.inner.lock().await;
                        inner.realtime_stt.take()
                    };
                    if let Some(rt) = rt {
                        rt.streaming_enabled.store(false, Ordering::Relaxed);
                        rt.uploader_task.abort();
                        rt.receiver_task.abort();
                        tauri::async_runtime::spawn(async move {
                            rt.handle.shutdown().await;
                        });
                    }

                    if let Err(e) = svc.cancel_recording().await {
                        self.mark_error(app, e.to_string()).await;
                        return ToggleResult {
                            stage: "error".into(),
                            final_text: None,
                            error: Some(e.to_string()),
                            is_recording: false,
                        };
                    }
                }

                #[cfg(not(any(windows, target_os = "macos")))]
                {
                    let _ = svc;
                }

                // Defensive: if we somehow still have a processing task, abort it.
                let task = if decision.abort_processing_task {
                    self.inner.lock().await.processing_task.take()
                } else {
                    None
                };
                if let Some(task) = task {
                    task.abort();
                }

                // Bump the session id so any pending work/hide from the previous session can't win.
                let session_id = {
                    let mut inner = self.inner.lock().await;
                    inner.session_id = decision.next_session_id;
                    inner.session_id
                };

                // Show first to avoid missing the stage update.
                Self::show_overlay(app);
                self.set_stage(app, SessionStage::Cancelled).await;

                {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        controller
                            .hide_overlay_if_session_matches(
                                &app_handle,
                                session_id,
                                Self::OVERLAY_HIDE_DELAY,
                            )
                            .await;
                    });
                }

                ToggleResult {
                    stage: "cancelled".into(),
                    final_text: None,
                    error: None,
                    is_recording: false,
                }
            }
            CancelDecision::Cancel(decision) => {
                let _ = svc;

                #[cfg(any(windows, target_os = "macos"))]
                {
                    // Defensive: if any realtime session is still around, shut it down.
                    let rt = {
                        let mut inner = self.inner.lock().await;
                        inner.realtime_stt.take()
                    };
                    if let Some(rt) = rt {
                        rt.streaming_enabled.store(false, Ordering::Relaxed);
                        rt.uploader_task.abort();
                        rt.receiver_task.abort();
                        tauri::async_runtime::spawn(async move {
                            rt.handle.shutdown().await;
                        });
                    }
                }

                // Invalidate the current session and abort the in-flight pipeline task.
                let (session_id, task) = {
                    let mut inner = self.inner.lock().await;
                    inner.session_id = decision.next_session_id;
                    (
                        inner.session_id,
                        if decision.abort_processing_task {
                            inner.processing_task.take()
                        } else {
                            None
                        },
                    )
                };

                if let Some(task) = task {
                    task.abort();
                }

                // Show first to avoid missing the stage update.
                Self::show_overlay(app);
                self.set_stage(app, SessionStage::Cancelled).await;

                {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        controller
                            .hide_overlay_if_session_matches(
                                &app_handle,
                                session_id,
                                Self::OVERLAY_HIDE_DELAY,
                            )
                            .await;
                    });
                }

                ToggleResult {
                    stage: "cancelled".into(),
                    final_text: None,
                    error: None,
                    is_recording: false,
                }
            }
            CancelDecision::NotRecording => {
                let _ = svc;
                self.set_status_message(app, "not recording".into(), Self::BUSY_TOAST_TTL)
                    .await;
                ToggleResult {
                    stage: "idle".into(),
                    final_text: None,
                    error: Some("not recording".into()),
                    is_recording: false,
                }
            }
        }
    }

    pub async fn toggle_recording<R: tauri::Runtime + 'static>(
        &self,
        app: &tauri::AppHandle<R>,
        svc: AppService,
    ) -> ToggleResult {
        let _transition_guard = match self.try_acquire_transition() {
            Some(g) => g,
            None => {
                self.set_status_message(app, "busy".into(), Self::BUSY_TOAST_TTL)
                    .await;
                return ToggleResult {
                    stage: "busy".into(),
                    final_text: None,
                    error: Some("busy".into()),
                    is_recording: self.inner.lock().await.stage == SessionStage::Recording,
                };
            }
        };

        self.maybe_pause_after_transition_acquired().await;

        // Minimal controller behavior:
        // - idle -> start recording
        // - recording -> stop and run
        // - busy -> ignore (for now)
        let stage = { self.inner.lock().await.stage };

        match stage {
            SessionStage::Idle
            | SessionStage::Error
            | SessionStage::Cancelled
            | SessionStage::Success => {
                // Show first so the overlay doesn't miss the stage update.
                Self::show_overlay(app);
                self.set_stage(app, SessionStage::Recording).await;

                // Snapshot the current session id for the watchdog.
                let session_id = { self.inner.lock().await.session_id };

                // Max-duration failsafe: stop recording automatically.
                // We use a dedicated OS thread + `block_on` here so we don't require the
                // controller future to be `Send`.
                {
                    let controller = self.clone();
                    let app_handle = app.clone();
                    let svc_for_watchdog = svc.clone();

                    std::thread::spawn(move || {
                        std::thread::sleep(Self::MAX_RECORDING_DURATION);

                        tauri::async_runtime::block_on(async move {
                            // Only auto-stop if we're still recording the same session.
                            let should_stop = {
                                let inner = controller.inner.lock().await;
                                inner.stage == SessionStage::Recording
                                    && inner.session_id == session_id
                            };

                            if should_stop {
                                let _ = controller
                                    .toggle_recording(&app_handle, svc_for_watchdog)
                                    .await;
                            }
                        });
                    });
                }

                #[cfg(any(windows, target_os = "macos"))]
                {
                    let controller = self.clone();
                    let app_handle = app.clone();

                    // If ElevenLabs realtime is selected, we will stream audio during recording
                    // and then run the post-STT pipeline with a transcript override on stop.
                    // NOTE: Use effective config so Power Mode profiles can enable realtime.
                    let mut wants_realtime = false;
                    let mut effective_language: Option<String> = None;

                    // IMPORTANT: if config loading fails here, we silently fall back to batch STT.
                    // Emit a warning + HUD notice so users can diagnose why realtime isn't active.
                    match svc.load_config() {
                        Ok(cfg) => {
                            let app_id = svc
                                .get_foreground_app()
                                .await
                                .unwrap_or_else(|_| voicewin_core::types::AppIdentity::new());
                            let eff = voicewin_core::power_mode::resolve_effective_config(
                                &cfg.defaults,
                                &cfg.profiles,
                                &app_id,
                                &voicewin_core::power_mode::EphemeralOverrides::default(),
                            );

                            wants_realtime = voicewin_core::stt::is_elevenlabs_realtime_selected(
                                &eff.stt_provider,
                                &eff.stt_model,
                            );
                            effective_language = Some(eff.language.clone());

                            log::info!(
                                "effective STT on recording start: provider={:?} model={:?} language={:?} profile={:?} wants_realtime={}",
                                eff.stt_provider,
                                eff.stt_model,
                                eff.language,
                                eff.matched_profile_name,
                                wants_realtime
                            );
                        }
                        Err(e) => {
                            log::warn!(
                                "load_config failed while starting recording; realtime STT disabled for this session (will use batch on stop): {e}"
                            );
                            controller
                                .set_status_message(
                                    &app_handle,
                                    format!(
                                        "Could not read config; realtime STT disabled (batch on stop). ({e})"
                                    ),
                                    Duration::from_millis(2500),
                                )
                                .await;
                        }
                    }

                    let eleven_key = if wants_realtime {
                        match get_secret(SecretKey::ElevenLabsApiKey) {
                            Ok(Some(v)) => v,
                            Ok(None) => String::new(),
                            Err(e) => {
                                let msg = format!(
                                    "ElevenLabs is selected but secret storage is unavailable. Open Settings -> ElevenLabs. ({e})"
                                );
                                controller.mark_error(&app_handle, msg.clone()).await;
                                return ToggleResult {
                                    stage: "error".into(),
                                    final_text: None,
                                    error: Some(msg),
                                    is_recording: false,
                                };
                            }
                        }
                    } else {
                        String::new()
                    };

                    if wants_realtime && eleven_key.trim().is_empty() {
                        let msg = "ElevenLabs is selected but no API key is set. Open Settings -> ElevenLabs.".to_string();
                        controller.mark_error(&app_handle, msg.clone()).await;
                        return ToggleResult {
                            stage: "error".into(),
                            final_text: None,
                            error: Some(msg),
                            is_recording: false,
                        };
                    }

                    // Realtime streaming plumbing.
                    let buffer_frames = (REALTIME_BUFFER_MS_DEFAULT / REALTIME_FRAME_MS_DEFAULT)
                        .max(1)
                        .min(400) as usize;

                    let streaming_enabled = Arc::new(AtomicBool::new(wants_realtime));
                    let dropped_chunks = Arc::new(AtomicU64::new(0));
                    let captured_frames = Arc::new(AtomicU64::new(0));
                    let sent_frames = Arc::new(AtomicU64::new(0));

                    // Default to a ~50ms frame at 48kHz; once the recorder is opened we will
                    // update this based on the actual device sample rate.
                    let frame_samples = Arc::new(AtomicUsize::new(2400));
                    let chunker = Arc::new(StdMutex::new(RealtimeFrameChunker::default()));

                    // Bounded, time-ish buffer: `buffer_frames * REALTIME_FRAME_MS`.
                    let (audio_tx, mut audio_rx) =
                        tokio::sync::mpsc::channel::<Vec<f32>>(buffer_frames);

                    struct LevelEmitState {
                        last_emit: Instant,
                        smoothed_rms: f32,
                        smoothed_peak: f32,
                    }

                    let level_state = Arc::new(std::sync::Mutex::new(LevelEmitState {
                        last_emit: Instant::now(),
                        smoothed_rms: 0.0,
                        smoothed_peak: 0.0,
                    }));

                    if let Err(e) = svc
                        .clone()
                        .start_recording_with_level_callback({
                            let level_state = level_state.clone();
                            let controller = controller.clone();
                            let app_handle = app_handle.clone();
                            let streaming_enabled = streaming_enabled.clone();
                            let dropped_chunks = dropped_chunks.clone();
                            let captured_frames = captured_frames.clone();
                            let frame_samples = frame_samples.clone();
                            let chunker = chunker.clone();
                            let audio_tx = audio_tx.clone();
                            move |chunk: &[f32]| {
                                let now = Instant::now();

                                // Realtime framing is done here so the callback->tokio bridge is time-based
                                // (fixed-ish frames) rather than "N driver chunks".
                                if streaming_enabled.load(Ordering::Relaxed) {
                                    let frame_len = frame_samples.load(Ordering::Relaxed).max(1);
                                    let frames = {
                                        let mut guard = match chunker.lock() {
                                            Ok(g) => g,
                                            Err(poisoned) => poisoned.into_inner(),
                                        };
                                        guard.push(chunk, frame_len)
                                    };

                                    for frame in frames {
                                        if audio_tx.try_send(frame).is_err() {
                                            // Bounded channel is full: realtime cannot keep up.
                                            dropped_chunks.fetch_add(1, Ordering::Relaxed);
                                            streaming_enabled.store(false, Ordering::Relaxed);
                                            log::warn!("realtime audio queue full; disabling realtime streaming");
                                            break;
                                        } else {
                                            captured_frames.fetch_add(1, Ordering::Relaxed);
                                        }
                                    }
                                }

                                let mut guard = match level_state.lock() {
                                    Ok(g) => g,
                                    Err(poisoned) => poisoned.into_inner(),
                                };

                                let dt = now.duration_since(guard.last_emit);
                                if dt < Duration::from_millis(50) {
                                    return;
                                }
                                guard.last_emit = now;

                                let (rms, peak) = compute_levels(chunk);
                                guard.smoothed_rms = smooth_level(guard.smoothed_rms, rms, dt);
                                guard.smoothed_peak = smooth_level(guard.smoothed_peak, peak, dt);

                                let (rms_out, peak_out) = (guard.smoothed_rms, guard.smoothed_peak);
                                drop(guard);

                                // The level callback is an `Fn`, so we must not move out of
                                // captured values. Clone handles for the async emit task.
                                let controller2 = controller.clone();
                                let app_handle2 = app_handle.clone();

                                tauri::async_runtime::spawn(async move {
                                    controller2
                                        .emit_mic_level(&app_handle2, rms_out, peak_out)
                                        .await;
                                });
                            }
                        })
                        .await
                    {
                        log::error!("start_recording failed: {e}");
                        let msg = voicewin_appcore::service::user_facing_audio_error(&e);
                        controller.mark_error(&app_handle, msg.clone()).await;
                        return ToggleResult {
                            stage: "error".into(),
                            final_text: None,
                            error: Some(msg),
                            is_recording: false,
                        };
                    }

                    // Start ElevenLabs realtime session after the recorder is opened, so we can
                    // determine the device sample rate.
                    if wants_realtime {
                        let sr = svc.recording_sample_rate_hz().await.unwrap_or(16_000);

                        // Update realtime framing to match the device sample rate.
                        let frame_len =
                            ((sr as usize) * (REALTIME_FRAME_MS_DEFAULT as usize) / 1000)
                            .max(1)
                            .min(sr as usize);
                        frame_samples.store(frame_len, Ordering::Relaxed);
                        log::info!(
                            "realtime audio framing: frame_ms={REALTIME_FRAME_MS_DEFAULT} frame_samples={frame_len} buffer_frames={buffer_frames} (~{}ms)",
                            buffer_frames as u64 * REALTIME_FRAME_MS_DEFAULT
                        );

                        log::info!(
                            "ElevenLabs realtime requested; starting WS session (sample_rate_hz={sr})"
                        );

                        let mut rt_cfg = match ElevenLabsRealtimeConfig::production(eleven_key, sr)
                        {
                            Ok(c) => c,
                            Err(e) => {
                                log::warn!("elevenlabs realtime disabled: {e}");
                                streaming_enabled.store(false, Ordering::Relaxed);
                                controller
                                    .set_status_message(
                                        &app_handle,
                                        format!("ElevenLabs realtime disabled: {e}"),
                                        Duration::from_millis(2500),
                                    )
                                    .await;
                                return ToggleResult {
                                    stage: "recording".into(),
                                    final_text: None,
                                    error: None,
                                    is_recording: true,
                                };
                            }
                        };

                        // Respect the current effective language selection.
                        let lang = effective_language.as_deref().unwrap_or("auto");
                        rt_cfg.language_code = match lang {
                            "auto" => None,
                            other => Some(other.to_string()),
                        };

                        log::info!(
                            "ElevenLabs realtime config: model_id={:?} commit_strategy={:?} language_code={:?}",
                            rt_cfg.model_id,
                            rt_cfg.commit_strategy,
                            rt_cfg.language_code
                        );

                        let connect_start = Instant::now();
                        match spawn_realtime_session(rt_cfg).await {
                            Ok((handle, mut events)) => {
                                log::info!(
                                    "ElevenLabs realtime WS session started (connect_ms={})",
                                    connect_start.elapsed().as_millis()
                                );

                                let last_error = Arc::new(StdMutex::new(None));
                                let last_warning = Arc::new(StdMutex::new(None));
                                let session_id_for_realtime =
                                    { controller.inner.lock().await.session_id };

                                let (uploader_cmd_tx, mut uploader_cmd_rx) =
                                    tokio::sync::mpsc::channel::<RealtimeUploaderCmd>(2);

                                // Uploader task: convert f32 -> PCM16 and stream to WS.
                                let handle_for_uploader = handle.clone();
                                let streaming_enabled_for_uploader = streaming_enabled.clone();
                                let sent_frames_for_uploader = sent_frames.clone();
                                let uploader_task = tauri::async_runtime::spawn(async move {
                                    let mut drain_waiter: Option<tokio::sync::oneshot::Sender<bool>> = None;
                                    let mut drain_ok = true;

                                    loop {
                                        if let Some(respond_to) = drain_waiter.take() {
                                            // Drain any currently queued frames without waiting.
                                            loop {
                                                match audio_rx.try_recv() {
                                                    Ok(frame) => {
                                                        let pcm = pcm_s16le_from_f32(&frame);
                                                        if !handle_for_uploader.send_audio_chunk(pcm).await {
                                                            drain_ok = false;
                                                            streaming_enabled_for_uploader
                                                                .store(false, Ordering::Relaxed);
                                                            break;
                                                        }
                                                        sent_frames_for_uploader.fetch_add(1, Ordering::Relaxed);
                                                    }
                                                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                                                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                                                }
                                            }

                                            let _ = respond_to.send(drain_ok);
                                            break;
                                        }

                                        tokio::select! {
                                            cmd = uploader_cmd_rx.recv() => {
                                                match cmd {
                                                    Some(RealtimeUploaderCmd::Drain { respond_to }) => {
                                                        drain_waiter = Some(respond_to);
                                                    }
                                                    None => break,
                                                }
                                            }
                                            frame = audio_rx.recv() => {
                                                let Some(frame) = frame else {
                                                    break;
                                                };
                                                let pcm = pcm_s16le_from_f32(&frame);
                                                if !handle_for_uploader.send_audio_chunk(pcm).await {
                                                    drain_ok = false;
                                                    log::warn!(
                                                        "ElevenLabs realtime uploader failed (websocket closed); disabling streaming"
                                                    );
                                                    // Realtime session died; disable streaming so the audio callback stops enqueueing.
                                                    streaming_enabled_for_uploader.store(false, Ordering::Relaxed);
                                                    break;
                                                }
                                                sent_frames_for_uploader.fetch_add(1, Ordering::Relaxed);
                                            }
                                        }
                                    }
                                });

                                // Receiver task: update overlay with live preview.
                                let receiver_controller = controller.clone();
                                let receiver_app = app_handle.clone();
                                let streaming_enabled_for_receiver = streaming_enabled.clone();
                                let last_error_for_receiver = last_error.clone();
                                let last_warning_for_receiver = last_warning.clone();
                                let receiver_task = tauri::async_runtime::spawn(async move {
                                    let mut last_emit = Instant::now();
                                    let mut saw_live_text = false;
                                    while let Some(evt) = events.recv().await {
                                        // Don't let stale realtime updates leak into a cancelled/new session.
                                        if receiver_controller.inner.lock().await.session_id
                                            != session_id_for_realtime
                                        {
                                            break;
                                        }

                                        match evt {
                                            RealtimeEvent::SessionStarted { session_id } => {
                                                // Avoid logging secrets; session id is safe and useful for correlating
                                                // server-side telemetry / support.
                                                log::info!(
                                                    "ElevenLabs realtime session started (session_id={session_id})"
                                                );
                                            }
                                            RealtimeEvent::LiveText { committed, partial } => {
                                                if !saw_live_text {
                                                    saw_live_text = true;
                                                    log::info!(
                                                        "ElevenLabs realtime received transcript updates (committed_len={} partial_len={})",
                                                        committed.trim().len(),
                                                        partial.trim().len()
                                                    );
                                                }

                                                let c = committed.trim();
                                                let p = partial.trim();
                                                let live = if c.is_empty() {
                                                    p.to_string()
                                                } else if p.is_empty() {
                                                    c.to_string()
                                                } else {
                                                    format!("{c} {p}")
                                                };
                                                // Throttle UI updates a bit.
                                                if last_emit.elapsed() < Duration::from_millis(200)
                                                {
                                                    continue;
                                                }
                                                last_emit = Instant::now();
                                                receiver_controller.set_last_text(Some(live)).await;
                                                receiver_controller
                                                    .emit_status(&receiver_app)
                                                    .await;
                                            }
                                            RealtimeEvent::Warning { kind: _, message } => {
                                                log::warn!("ElevenLabs realtime warning: {message}");
                                                // Persist the latest warning so stop-time History can reflect it.
                                                if let Ok(mut guard) =
                                                    last_warning_for_receiver.lock()
                                                {
                                                    *guard = Some(message.clone());
                                                }
                                                receiver_controller
                                                    .set_status_message(
                                                        &receiver_app,
                                                        message,
                                                        Duration::from_millis(2500),
                                                    )
                                                    .await;
                                            }
                                            RealtimeEvent::Error {
                                                message_type,
                                                error,
                                            } => {
                                                log::warn!(
                                                    "ElevenLabs realtime error ({message_type}): {error}"
                                                );
                                                // Stop feeding realtime immediately; we'll fall back to batch on stop.
                                                streaming_enabled_for_receiver
                                                    .store(false, Ordering::Relaxed);

                                                // Store a concise detail for stop-time warnings.
                                                if let Ok(mut guard) =
                                                    last_error_for_receiver.lock()
                                                {
                                                    *guard =
                                                        Some(format!("{message_type}: {error}"));
                                                }

                                                receiver_controller
                                                    .set_status_message(
                                                        &receiver_app,
                                                        format!("ElevenLabs realtime error ({message_type}): {error}"),
                                                        Duration::from_millis(2500),
                                                    )
                                                    .await;

                                                // Avoid spamming the HUD if more errors arrive.
                                                break;
                                            }
                                        }
                                    }
                                });

                                // Store realtime state for stop/cancel.
                                {
                                    let mut inner = controller.inner.lock().await;
                                    inner.realtime_stt = Some(RealtimeSttState {
                                        handle,
                                        uploader_cmd_tx,
                                        uploader_task,
                                        receiver_task,
                                        streaming_enabled: streaming_enabled.clone(),
                                        dropped_chunks: dropped_chunks.clone(),
                                        captured_frames: captured_frames.clone(),
                                        sent_frames: sent_frames.clone(),
                                        audio_tx: audio_tx.clone(),
                                        chunker: chunker.clone(),
                                        last_error,
                                        last_warning,
                                    });
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "failed to start ElevenLabs realtime; will fall back to batch on stop: {e}"
                                );
                                // Some log viewers filter to INFO only; duplicate the key detail.
                                log::info!(
                                    "ElevenLabs realtime start failed; using batch on stop: {e}"
                                );
                                streaming_enabled.store(false, Ordering::Relaxed);
                                controller
                                    .set_status_message(
                                        &app_handle,
                                        format!("ElevenLabs realtime unavailable; will use batch on stop. ({e})"),
                                        Duration::from_secs(6),
                                    )
                                    .await;
                            }
                        }
                    }
                }

                ToggleResult {
                    stage: "recording".into(),
                    final_text: None,
                    error: None,
                    is_recording: true,
                }
            }
            SessionStage::Recording => {
                // Show first so the overlay doesn't miss the stage update.
                Self::show_overlay(app);

                #[cfg(any(windows, target_os = "macos"))]
                {
                    // Stop any realtime streaming for this session.
                    let realtime = {
                        let mut inner = self.inner.lock().await;
                        inner.realtime_stt.take()
                    };

                    if realtime.is_some() {
                        self.set_stage(app, SessionStage::Finalizing).await;
                    } else {
                        self.set_stage(app, SessionStage::Transcribing).await;
                    }

                    if let Some(rt) = realtime.as_ref() {
                        rt.streaming_enabled.store(false, Ordering::Relaxed);
                        // Stop feeding new audio frames from the mic callback.
                    }

                    let audio = match svc.clone().stop_recording().await {
                        Ok(a) => a,
                        Err(e) => {
                            log::error!("stop_recording failed: {e}");
                            self.mark_error(app, e.to_string()).await;
                            return ToggleResult {
                                stage: "error".into(),
                                final_text: None,
                                error: Some(e.to_string()),
                                is_recording: false,
                            };
                        }
                    };

                    let n = audio.samples.len();
                    let ms = (n as f64 / 16_000.0) * 1000.0;
                    log::info!("captured audio: {n} samples (~{ms:.0}ms)");
                    if n < 160 {
                        let msg = "No audio captured from the microphone.".to_string();
                        self.mark_error(app, msg.clone()).await;
                        return ToggleResult {
                            stage: "error".into(),
                            final_text: None,
                            error: Some(msg),
                            is_recording: false,
                        };
                    }

                    // Snapshot the current session id so a later Cancel can invalidate results.
                    let session_id = { self.inner.lock().await.session_id };

                    // Run the session pipeline in a background task so the UI remains responsive
                    // and the Cancel button can abort the in-flight work.
                    let controller = self.clone();
                    let app_handle = app.clone();
                    let svc_for_task = svc.clone();

                    let handle = tauri::async_runtime::spawn(async move {
                        let controller_for_hook = controller.clone();
                        let app_for_hook = app_handle.clone();

                        // If we were running ElevenLabs realtime, try to finalize and produce a transcript override.
                        // If it fails, fall back to batch STT using the captured audio.
                        let mut transcript_override = String::new();
                        let mut warning: Option<String> = None;

                        if let Some(rt) = realtime {
                            let finalize_budget = Duration::from_secs(2);
                            let finalize_started = Instant::now();

                            let captured = rt.captured_frames.load(Ordering::Relaxed);
                            let sent = rt.sent_frames.load(Ordering::Relaxed);
                            let backlog_frames = captured.saturating_sub(sent);
                            log::info!(
                                "ElevenLabs realtime stop: captured_frames={captured} sent_frames={sent} backlog_frames={backlog_frames} (~{}ms)",
                                backlog_frames as u64 * REALTIME_FRAME_MS_DEFAULT
                            );

                            let dropped = rt.dropped_chunks.load(Ordering::Relaxed);
                            let provider_warning = rt.last_warning.lock().ok().and_then(|guard| guard.clone());
                            if let Some(w) = provider_warning.as_ref() {
                                log::warn!("ElevenLabs realtime warning (recording): {w}");
                            }
                            let provider_error = rt.last_error.lock().ok().and_then(|guard| guard.clone());
                            let preflight = realtime_preflight_decision(
                                dropped,
                                provider_warning.as_deref(),
                                provider_error.as_deref(),
                            );
                            let mut allow_override = preflight.allow_override;
                            warning = preflight.warning;

                            if dropped > 0 {
                                let msg = format!(
                                    "ElevenLabs realtime dropped {dropped} audio chunks; transcript may be incomplete."
                                );
                                log::warn!("{msg}");
                                controller
                                    .set_status_message(
                                        &app_handle,
                                        msg,
                                        Duration::from_millis(2500),
                                    )
                                    .await;
                            }

                            // Catch up: flush the final partial frame, drain queued audio, then finalize.
                            if allow_override {
                                // Flush the final partial frame into the queue so it can be uploaded.
                                let tail = {
                                    let mut guard = match rt.chunker.lock() {
                                        Ok(g) => g,
                                        Err(poisoned) => poisoned.into_inner(),
                                    };
                                    guard.flush()
                                };

                                if let Some(tail) = tail {
                                    let remaining = finalize_budget
                                        .checked_sub(finalize_started.elapsed())
                                        .unwrap_or_default();
                                    if remaining.is_zero() {
                                        allow_override = false;
                                    } else {
                                        let queued = tokio::time::timeout(
                                            remaining,
                                            rt.audio_tx.send(tail),
                                        )
                                        .await;
                                        if !matches!(queued, Ok(Ok(()))) {
                                            allow_override = false;
                                            warning = resolve_realtime_finalize_outcome(
                                                warning.take(),
                                                RealtimeFinalizeOutcome::QueueTailFailed,
                                            )
                                            .warning;
                                        }
                                    }
                                }
                            }

                            if allow_override {
                                let remaining = finalize_budget
                                    .checked_sub(finalize_started.elapsed())
                                    .unwrap_or_default();
                                if remaining.is_zero() {
                                    allow_override = false;
                                } else {
                                    // Ask the uploader to drain queued frames (time-bounded).
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    if rt
                                        .uploader_cmd_tx
                                        .send(RealtimeUploaderCmd::Drain { respond_to: tx })
                                        .await
                                        .is_err()
                                    {
                                        allow_override = false;
                                        warning = resolve_realtime_finalize_outcome(
                                            warning.take(),
                                            RealtimeFinalizeOutcome::UploaderNotRunning,
                                        )
                                        .warning;
                                    } else {
                                        let drain_start = Instant::now();
                                        match tokio::time::timeout(remaining, rx).await {
                                            Ok(Ok(ok)) if ok => {}
                                            Ok(Ok(_)) => {
                                                allow_override = false;
                                                warning = resolve_realtime_finalize_outcome(
                                                    warning.take(),
                                                    RealtimeFinalizeOutcome::DrainFailed,
                                                )
                                                .warning;
                                            }
                                            Ok(Err(_)) => {
                                                allow_override = false;
                                                warning = resolve_realtime_finalize_outcome(
                                                    warning.take(),
                                                    RealtimeFinalizeOutcome::DrainCancelled,
                                                )
                                                .warning;
                                            }
                                            Err(_) => {
                                                allow_override = false;
                                                warning = resolve_realtime_finalize_outcome(
                                                    warning.take(),
                                                    RealtimeFinalizeOutcome::DrainTimedOut,
                                                )
                                                .warning;
                                            }
                                        }

                                        log::info!(
                                            "ElevenLabs realtime drain finished: ok={allow_override} elapsed_ms={}",
                                            drain_start.elapsed().as_millis()
                                        );
                                    }
                                }
                            }

                            if allow_override {
                                let remaining = finalize_budget
                                    .checked_sub(finalize_started.elapsed())
                                    .unwrap_or_default();
                                if remaining.is_zero() {
                                    allow_override = false;
                                } else {
                                    log::info!("ElevenLabs realtime finalize started");
                                    match tokio::time::timeout(remaining, rt.handle.finalize()).await {
                                        Ok(Ok(t)) => {
                                            let decision = resolve_realtime_finalize_outcome(
                                                warning.take(),
                                                RealtimeFinalizeOutcome::FinalizeText(t),
                                            );
                                            warning = decision.warning;
                                            if !decision.transcript_override.trim().is_empty() {
                                                let t = decision.transcript_override;
                                                log::info!(
                                                    "ElevenLabs realtime finalize ok; using transcript override (chars={})",
                                                    t.trim().len()
                                                );
                                                transcript_override = t;
                                                log::info!(
                                                    "ElevenLabs realtime stop finished: used_override=true elapsed_ms={}",
                                                    finalize_started.elapsed().as_millis()
                                                );
                                            } else {
                                                let msg = "ElevenLabs realtime produced no text; using batch on stop.".to_string();
                                                log::warn!("{msg}");
                                                controller
                                                    .set_status_message(
                                                        &app_handle,
                                                        msg,
                                                        Duration::from_millis(2500),
                                                    )
                                                    .await;
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            let detail = rt
                                                .last_error
                                                .lock()
                                                .ok()
                                                .and_then(|g| g.clone())
                                                .unwrap_or_else(|| e.to_string());
                                            warning = resolve_realtime_finalize_outcome(
                                                warning.take(),
                                                RealtimeFinalizeOutcome::FinalizeError(detail.clone()),
                                            )
                                            .warning;
                                            let msg = format!(
                                                "ElevenLabs realtime failed; using batch on stop. ({detail})"
                                            );
                                            log::warn!("{msg}");
                                            controller
                                                .set_status_message(
                                                    &app_handle,
                                                    msg,
                                                    Duration::from_millis(2500),
                                                )
                                                .await;
                                        }
                                        Err(_) => {
                                            warning = resolve_realtime_finalize_outcome(
                                                warning.take(),
                                                RealtimeFinalizeOutcome::FinalizeTimedOut,
                                            )
                                            .warning;
                                            let msg = "ElevenLabs realtime could not finalize in 2s; using batch on stop.".to_string();
                                            log::warn!("{msg}");
                                            controller
                                                .set_status_message(
                                                    &app_handle,
                                                    msg,
                                                    Duration::from_millis(2500),
                                                )
                                                .await;
                                        }
                                    }
                                }
                            }

                            if transcript_override.trim().is_empty() {
                                log::info!(
                                    "ElevenLabs realtime stop finished: used_override=false elapsed_ms={}",
                                    finalize_started.elapsed().as_millis()
                                );
                            }

                            rt.receiver_task.abort();
                            rt.uploader_task.abort();
                            rt.handle.shutdown().await;
                        }

                        let using_override = !transcript_override.trim().is_empty();

                        let res = svc_for_task
                            .clone()
                            .run_session_with_hook(
                                voicewin_runtime::ipc::RunSessionRequest {
                                    transcript: transcript_override,
                                    warning,
                                },
                                audio,
                                move |stage| {
                                    let controller_for_hook = controller_for_hook.clone();
                                    let app_for_hook = app_for_hook.clone();
                                    async move {
                                        if let Some(stage) =
                                            overlay_stage_for_engine_stage(stage, using_override)
                                        {
                                            controller_for_hook
                                                .set_stage(&app_for_hook, stage)
                                                .await;
                                        }
                                    }
                                },
                            )
                            .await;

                        // Mark the background task as finished (best-effort).
                        {
                            let mut inner = controller.inner.lock().await;
                            inner.processing_task = None;
                        }

                        let current_session_id = { controller.inner.lock().await.session_id };
                        let decision = resolve_session_result(
                            current_session_id,
                            session_id,
                            res.map_err(|e| e.to_string()),
                        );

                        let _ = controller
                            .apply_session_result(&app_handle, session_id, decision)
                            .await;
                    });

                    {
                        let mut inner = self.inner.lock().await;
                        if let Some(prev) = inner.processing_task.take() {
                            prev.abort();
                        }
                        inner.processing_task = Some(handle);
                    }

                    ToggleResult {
                        stage: "transcribing".into(),
                        final_text: None,
                        error: None,
                        is_recording: false,
                    }
                }

                #[cfg(not(any(windows, target_os = "macos")))]
                {
                    let _ = svc;
                    self.set_stage(app, SessionStage::Error).await;
                    self.set_status_message(
                        app,
                        "recording supported on Windows and macOS".into(),
                        Duration::from_secs(3),
                    )
                    .await;
                    ToggleResult {
                        stage: "error".into(),
                        final_text: None,
                        error: Some("recording supported on Windows and macOS".into()),
                        is_recording: false,
                    }
                }
            }
            _ => {
                // Busy.
                self.set_status_message(app, "busy".into(), Self::BUSY_TOAST_TTL)
                    .await;
                ToggleResult {
                    stage: "busy".into(),
                    final_text: None,
                    error: Some("busy".into()),
                    is_recording: stage == SessionStage::Recording,
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToggleResult {
    pub stage: String,
    pub final_text: Option<String>,
    pub error: Option<String>,
    pub is_recording: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RealtimePreflightDecision {
    allow_override: bool,
    warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RealtimeFinalizeDecision {
    transcript_override: String,
    warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionResultSuccessDecision {
    final_text: Option<String>,
    status_message: Option<String>,
    hide_delay: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CancelSessionDecision {
    next_session_id: u64,
    abort_processing_task: bool,
    should_cancel_recorder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CancelDecision {
    Cancel(CancelSessionDecision),
    NotRecording,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionResultDecision {
    Ignore,
    Success(SessionResultSuccessDecision),
    Failed {
        final_text: Option<String>,
        status_message: String,
    },
    Error {
        status_message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionResultSideEffect {
    Ignore,
    ShowOverlay,
    HideOverlayAfter(Duration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionResultApplyOutcome {
    side_effect: SessionResultSideEffect,
    log_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
enum RealtimeFinalizeOutcome {
    FinalizeText(String),
    QueueTailFailed,
    UploaderNotRunning,
    DrainFailed,
    DrainCancelled,
    DrainTimedOut,
    FinalizeError(String),
    FinalizeTimedOut,
}

fn merge_warning(dst: &mut Option<String>, msg: impl AsRef<str>) {
    let msg = msg.as_ref().trim().to_string();
    if msg.is_empty() {
        return;
    }

    *dst = match dst.take() {
        Some(existing) if !existing.trim().is_empty() => Some(format!("{existing} | {msg}")),
        _ => Some(msg),
    };
}

fn warning_disables_realtime_override(msg: &str) -> bool {
    let lowered = msg.to_lowercase();
    lowered.contains("dropped") || lowered.contains("backpressure")
}

fn realtime_preflight_decision(
    dropped_chunks: u64,
    provider_warning: Option<&str>,
    provider_error: Option<&str>,
) -> RealtimePreflightDecision {
    let mut warning = None;
    let mut allow_override = true;

    if dropped_chunks > 0 {
        merge_warning(
            &mut warning,
            format!(
                "ElevenLabs realtime dropped {dropped_chunks} audio chunks; transcript may be incomplete."
            ),
        );
        allow_override = false;
    }

    if let Some(provider_warning) = provider_warning.filter(|msg| !msg.trim().is_empty()) {
        if warning_disables_realtime_override(provider_warning) {
            allow_override = false;
        }
        merge_warning(&mut warning, provider_warning);
    }

    if provider_error.is_some_and(|msg| !msg.trim().is_empty()) {
        allow_override = false;
    }

    RealtimePreflightDecision {
        allow_override,
        warning,
    }
}

fn resolve_realtime_finalize_outcome(
    initial_warning: Option<String>,
    outcome: RealtimeFinalizeOutcome,
) -> RealtimeFinalizeDecision {
    let mut warning = initial_warning;
    let transcript_override = match outcome {
        RealtimeFinalizeOutcome::FinalizeText(text) => {
            if let Some(text) = voicewin_core::stt::accept_transcript_override(text) {
                text
            } else {
                merge_warning(
                    &mut warning,
                    "ElevenLabs realtime produced no text; using batch on stop.",
                );
                String::new()
            }
        }
        RealtimeFinalizeOutcome::QueueTailFailed => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime could not queue final audio frame; using batch on stop.",
            );
            String::new()
        }
        RealtimeFinalizeOutcome::UploaderNotRunning => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime uploader is not running; using batch on stop.",
            );
            String::new()
        }
        RealtimeFinalizeOutcome::DrainFailed => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime uploader failed while draining; using batch on stop.",
            );
            String::new()
        }
        RealtimeFinalizeOutcome::DrainCancelled => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime uploader drain cancelled; using batch on stop.",
            );
            String::new()
        }
        RealtimeFinalizeOutcome::DrainTimedOut => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime could not catch up in 2s; using batch on stop.",
            );
            String::new()
        }
        RealtimeFinalizeOutcome::FinalizeError(detail) => {
            merge_warning(
                &mut warning,
                format!("ElevenLabs realtime failed; using batch on stop. ({detail})"),
            );
            String::new()
        }
        RealtimeFinalizeOutcome::FinalizeTimedOut => {
            merge_warning(
                &mut warning,
                "ElevenLabs realtime could not finalize in 2s; using batch on stop.",
            );
            String::new()
        }
    };

    RealtimeFinalizeDecision {
        transcript_override,
        warning,
    }
}

fn overlay_stage_for_engine_stage(stage: &str, using_override: bool) -> Option<SessionStage> {
    match stage {
        "transcribing" => Some(if using_override {
            SessionStage::Finalizing
        } else {
            SessionStage::Transcribing
        }),
        "enhancing" => Some(SessionStage::Enhancing),
        "inserting" => Some(SessionStage::Inserting),
        _ => None,
    }
}

fn resolve_session_result(
    current_session_id: u64,
    result_session_id: u64,
    result: Result<voicewin_runtime::ipc::RunSessionResponse, String>,
) -> SessionResultDecision {
    if current_session_id != result_session_id {
        return SessionResultDecision::Ignore;
    }

    match result {
        Ok(r) if r.stage == "done" => {
            let status_message = r.error.filter(|s| !s.trim().is_empty());
            let hide_delay = if status_message.is_some() {
                Duration::from_millis(2500)
            } else {
                SessionController::OVERLAY_HIDE_DELAY
            };

            SessionResultDecision::Success(SessionResultSuccessDecision {
                final_text: r.final_text,
                status_message,
                hide_delay,
            })
        }
        Ok(r) if r.stage == "failed" => SessionResultDecision::Failed {
            final_text: r.final_text,
            status_message: r
                .error
                .unwrap_or_else(|| "Could not insert. Saved to History.".into()),
        },
        Ok(r) if r.stage == "error" => SessionResultDecision::Error {
            status_message: r.error.unwrap_or_else(|| "Session failed".into()),
        },
        Ok(r) => SessionResultDecision::Error {
            status_message: r
                .error
                .unwrap_or_else(|| format!("Session ended in unexpected stage: {}", r.stage)),
        },
        Err(error) => SessionResultDecision::Error {
            status_message: error,
        },
    }
}

fn apply_stage_transition(inner: &mut Inner, stage: SessionStage) {
    let prev = inner.stage;

    if inner.stage == SessionStage::Recording && stage != SessionStage::Recording {
        if let Some(start) = inner.recording_started_at {
            inner.recording_elapsed_ms = Some(start.elapsed().as_millis() as u64);
        }
        inner.recording_started_at = None;
    }

    inner.stage = stage;

    if prev != stage {
        log::info!("session stage: {:?} -> {:?}", prev, stage);
    }

    if stage == SessionStage::Recording {
        inner.session_id = inner.session_id.wrapping_add(1);
        inner.recording_started_at = Some(Instant::now());
        inner.recording_elapsed_ms = None;
        inner.last_text = None;
        inner.status_message = None;
        inner.status_message_expires_at = None;
    }

    if stage == SessionStage::Idle {
        inner.recording_started_at = None;
        inner.recording_elapsed_ms = None;
        inner.status_message = None;
        inner.status_message_expires_at = None;
    }
}

fn apply_session_result_to_inner(
    inner: &mut Inner,
    session_id: u64,
    decision: SessionResultDecision,
) -> SessionResultApplyOutcome {
    if inner.session_id != session_id {
        return SessionResultApplyOutcome {
            side_effect: SessionResultSideEffect::Ignore,
            log_message: None,
        };
    }

    match decision {
        SessionResultDecision::Ignore => SessionResultApplyOutcome {
            side_effect: SessionResultSideEffect::Ignore,
            log_message: None,
        },
        SessionResultDecision::Success(success) => {
            inner.last_text = success.final_text;
            if let Some(msg) = success.status_message {
                inner.status_message = Some(msg);
                inner.status_message_expires_at = Some(Instant::now() + success.hide_delay);
            }
            apply_stage_transition(inner, SessionStage::Success);
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::HideOverlayAfter(success.hide_delay),
                log_message: None,
            }
        }
        SessionResultDecision::Failed {
            final_text,
            status_message,
        } => {
            inner.last_text = final_text;
            apply_stage_transition(inner, SessionStage::Error);
            inner.status_message = Some(status_message.clone());
            inner.status_message_expires_at = Some(Instant::now() + Duration::from_secs(6));
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::ShowOverlay,
                log_message: Some(format!("session failed stage=failed: {status_message}")),
            }
        }
        SessionResultDecision::Error { status_message } => {
            apply_stage_transition(inner, SessionStage::Error);
            inner.status_message = Some(status_message.clone());
            inner.status_message_expires_at = Some(Instant::now() + Duration::from_secs(6));
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::ShowOverlay,
                log_message: Some(format!("session error: {status_message}")),
            }
        }
    }
}

fn resolve_cancel_decision(
    stage: SessionStage,
    current_session_id: u64,
    has_processing_task: bool,
) -> CancelDecision {
    match stage {
        SessionStage::Recording => CancelDecision::Cancel(CancelSessionDecision {
            next_session_id: current_session_id.wrapping_add(1),
            abort_processing_task: has_processing_task,
            should_cancel_recorder: true,
        }),
        SessionStage::Finalizing
        | SessionStage::Transcribing
        | SessionStage::Enhancing
        | SessionStage::Inserting => CancelDecision::Cancel(CancelSessionDecision {
            next_session_id: current_session_id.wrapping_add(1),
            abort_processing_task: has_processing_task,
            should_cancel_recorder: false,
        }),
        _ => CancelDecision::NotRecording,
    }
}

fn stage_label(stage: SessionStage) -> &'static str {
    match stage {
        SessionStage::Idle => "idle",
        SessionStage::Recording => "recording",
        SessionStage::Finalizing => "finalizing",
        SessionStage::Transcribing => "transcribing",
        SessionStage::Enhancing => "enhancing",
        SessionStage::Inserting => "inserting",
        SessionStage::Success => "success",
        SessionStage::Error => "error",
        SessionStage::Cancelled => "cancelled",
    }
}

fn preview_text(text: &str) -> String {
    const MAX: usize = 120;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }

    trimmed.chars().take(MAX).collect::<String>() + "…"
}

// Aggregates variable-sized mic chunks into fixed-ish frames so:
// - we drastically reduce per-message overhead (channel + encode + JSON)
// - the bounded queue becomes time-based ("~N seconds" rather than "N chunks")
//
// This is intentionally simple and allocation-heavy; correctness + bounded memory first.
#[derive(Debug, Default)]
struct RealtimeFrameChunker {
    buf: Vec<f32>,
    start: usize,
}

impl RealtimeFrameChunker {
    fn push(&mut self, chunk: &[f32], frame_samples: usize) -> Vec<Vec<f32>> {
        if chunk.is_empty() || frame_samples == 0 {
            return Vec::new();
        }

        self.buf.extend_from_slice(chunk);

        let mut out = Vec::new();
        while self.buf.len().saturating_sub(self.start) >= frame_samples {
            let end = self.start + frame_samples;
            out.push(self.buf[self.start..end].to_vec());
            self.start = end;
        }

        // Avoid unbounded growth from a large `start` offset.
        if self.start > 0 && (self.start >= frame_samples.saturating_mul(4) || self.start >= 8192) {
            self.buf.drain(..self.start);
            self.start = 0;
        }

        out
    }

    fn flush(&mut self) -> Option<Vec<f32>> {
        let remaining = self.buf.len().saturating_sub(self.start);
        if remaining == 0 {
            self.buf.clear();
            self.start = 0;
            return None;
        }

        let out = self.buf[self.start..].to_vec();
        self.buf.clear();
        self.start = 0;
        Some(out)
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn pcm_s16le_from_f32(samples: &[f32]) -> Vec<u8> {
    // Convert mono float samples to PCM16 little-endian bytes for ElevenLabs realtime.
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = s.clamp(-1.0, 1.0);
        let i = (v * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&i.to_le_bytes());
    }
    out
}

#[allow(dead_code)]
pub fn compute_levels(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f32;

    for &s in samples {
        let v = s.abs();
        if v > peak {
            peak = v;
        }
        sum_sq += s * s;
    }

    let rms = (sum_sq / samples.len() as f32).sqrt();
    (rms.clamp(0.0, 1.0), peak.clamp(0.0, 1.0))
}

#[allow(dead_code)]
pub fn smooth_level(prev: f32, next: f32, dt: Duration) -> f32 {
    // Exponential smoothing with a 150ms time constant.
    let tau = 0.15f32;
    let alpha = 1.0 - (-dt.as_secs_f32() / tau).exp();
    prev + (next - prev) * alpha
}

#[cfg(test)]
mod tests {
    use super::SessionController;
    use super::RealtimeFrameChunker;
    use super::{
        Inner, SessionResultApplyOutcome, SessionResultSideEffect, CancelDecision,
        CancelSessionDecision,
        RealtimeFinalizeDecision, RealtimeFinalizeOutcome, SessionResultDecision,
        SessionResultSuccessDecision, SessionStage, apply_session_result_to_inner,
        overlay_stage_for_engine_stage, realtime_preflight_decision, resolve_cancel_decision,
        resolve_realtime_finalize_outcome, resolve_session_result,
    };
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use std::time::Duration;
    use tempfile::tempdir;
    use voicewin_appcore::service::AppService;
    use voicewin_core::types::AppIdentity;
    use voicewin_platform::test::{MemoryInserter, TestContextProvider};
    use voicewin_runtime::ipc::RunSessionResponse;

    fn test_service() -> AppService {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let ctx = TestContextProvider::new(AppIdentity::new(), Default::default()).boxed();
        let inserter = Arc::new(MemoryInserter::default());
        AppService::new(config_path, ctx, inserter)
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn transition_gate_blocks_parallel_entries() {
        let controller = SessionController::new();

        let gate = controller.try_acquire_transition();
        assert!(gate.is_some());
        assert!(controller.try_acquire_transition().is_none());

        drop(gate);
        assert!(controller.try_acquire_transition().is_some());
    }

    #[test]
    fn realtime_chunker_frames_and_flushes_tail() {
        let mut c = RealtimeFrameChunker::default();

        // Frame size 3.
        let out1 = c.push(&[1.0, 2.0], 3);
        assert!(out1.is_empty());

        let out2 = c.push(&[3.0, 4.0, 5.0, 6.0, 7.0], 3);
        assert_eq!(out2.len(), 2);
        assert_eq!(out2[0], vec![1.0, 2.0, 3.0]);
        assert_eq!(out2[1], vec![4.0, 5.0, 6.0]);

        let tail = c.flush().unwrap();
        assert_eq!(tail, vec![7.0]);
        assert!(c.flush().is_none());
    }

    #[allow(dead_code)]
    fn assert_emit_mic_level_accepts_generic_app_handle<R: tauri::Runtime + 'static>(
        controller: &SessionController,
        app: &tauri::AppHandle<R>,
    ) {
        let _future = controller.emit_mic_level(app, 0.25, 0.5);
    }

    #[test]
    fn realtime_preflight_drops_force_batch_and_warning() {
        let decision = realtime_preflight_decision(3, None, None);

        assert!(!decision.allow_override);
        assert_eq!(
            decision.warning.as_deref(),
            Some("ElevenLabs realtime dropped 3 audio chunks; transcript may be incomplete.")
        );
    }

    #[test]
    fn realtime_preflight_provider_backpressure_warning_forces_batch() {
        let decision = realtime_preflight_decision(
            0,
            Some("Provider reported backpressure drops"),
            None,
        );

        assert!(!decision.allow_override);
        assert_eq!(
            decision.warning.as_deref(),
            Some("Provider reported backpressure drops")
        );
    }

    #[test]
    fn realtime_finalize_resolution_uses_override_for_non_empty_text() {
        let decision = resolve_realtime_finalize_outcome(
            None,
            RealtimeFinalizeOutcome::FinalizeText("hello from realtime".into()),
        );

        assert_eq!(
            decision,
            RealtimeFinalizeDecision {
                transcript_override: "hello from realtime".into(),
                warning: None,
            }
        );
    }

    #[test]
    fn realtime_finalize_resolution_empty_text_falls_back_with_warning() {
        let decision = resolve_realtime_finalize_outcome(
            Some("Provider warning".into()),
            RealtimeFinalizeOutcome::FinalizeText("   ".into()),
        );

        assert_eq!(decision.transcript_override, "");
        assert_eq!(
            decision.warning.as_deref(),
            Some("Provider warning | ElevenLabs realtime produced no text; using batch on stop.")
        );
    }

    #[test]
    fn realtime_finalize_error_merges_after_existing_warning() {
        let decision = resolve_realtime_finalize_outcome(
            Some("Provider warning".into()),
            RealtimeFinalizeOutcome::FinalizeError("quota exceeded".into()),
        );

        assert_eq!(decision.transcript_override, "");
        assert_eq!(
            decision.warning.as_deref(),
            Some("Provider warning | ElevenLabs realtime failed; using batch on stop. (quota exceeded)")
        );
    }

    #[test]
    fn realtime_finalize_timeout_merges_after_existing_warning() {
        let decision = resolve_realtime_finalize_outcome(
            Some("Provider warning".into()),
            RealtimeFinalizeOutcome::FinalizeTimedOut,
        );

        assert_eq!(decision.transcript_override, "");
        assert_eq!(
            decision.warning.as_deref(),
            Some("Provider warning | ElevenLabs realtime could not finalize in 2s; using batch on stop.")
        );
    }

    #[test]
    fn engine_stage_mapping_uses_finalizing_when_override_is_active() {
        assert_eq!(
            overlay_stage_for_engine_stage("transcribing", true),
            Some(SessionStage::Finalizing)
        );
        assert_eq!(
            overlay_stage_for_engine_stage("transcribing", false),
            Some(SessionStage::Transcribing)
        );
        assert_eq!(
            overlay_stage_for_engine_stage("enhancing", false),
            Some(SessionStage::Enhancing)
        );
        assert_eq!(
            overlay_stage_for_engine_stage("inserting", false),
            Some(SessionStage::Inserting)
        );
        assert_eq!(overlay_stage_for_engine_stage("unknown", false), None);
    }

    #[test]
    fn stale_session_result_is_ignored() {
        let decision = resolve_session_result(
            8,
            7,
            Ok(RunSessionResponse {
                stage: "done".into(),
                final_text: Some("late transcript".into()),
                error: None,
            }),
        );

        assert_eq!(decision, SessionResultDecision::Ignore);
    }

    #[test]
    fn done_session_result_keeps_success_and_optional_warning() {
        let decision = resolve_session_result(
            7,
            7,
            Ok(RunSessionResponse {
                stage: "done".into(),
                final_text: Some("final text".into()),
                error: Some("Enhancement failed; inserted raw transcript.".into()),
            }),
        );

        assert_eq!(
            decision,
            SessionResultDecision::Success(SessionResultSuccessDecision {
                final_text: Some("final text".into()),
                status_message: Some("Enhancement failed; inserted raw transcript.".into()),
                hide_delay: Duration::from_millis(2500),
            })
        );
    }

    #[test]
    fn failed_session_result_preserves_error_message() {
        let decision = resolve_session_result(
            4,
            4,
            Ok(RunSessionResponse {
                stage: "failed".into(),
                final_text: Some("recoverable text".into()),
                error: Some("Accessibility permissions required".into()),
            }),
        );

        assert_eq!(
            decision,
            SessionResultDecision::Failed {
                final_text: Some("recoverable text".into()),
                status_message: "Accessibility permissions required".into(),
            }
        );
    }

    #[test]
    fn failed_session_result_uses_history_fallback_message_when_missing() {
        let decision = resolve_session_result(
            4,
            4,
            Ok(RunSessionResponse {
                stage: "failed".into(),
                final_text: Some("recoverable text".into()),
                error: None,
            }),
        );

        assert_eq!(
            decision,
            SessionResultDecision::Failed {
                final_text: Some("recoverable text".into()),
                status_message: "Could not insert. Saved to History.".into(),
            }
        );
    }

    #[test]
    fn error_stage_session_result_preserves_error_message() {
        let decision = resolve_session_result(
            4,
            4,
            Ok(RunSessionResponse {
                stage: "error".into(),
                final_text: None,
                error: Some("engine pipeline failed".into()),
            }),
        );

        assert_eq!(
            decision,
            SessionResultDecision::Error {
                status_message: "engine pipeline failed".into(),
            }
        );
    }

    #[test]
    fn matching_success_result_applies_all_fields_atomically() {
        let mut inner = Inner {
            stage: SessionStage::Transcribing,
            session_id: 7,
            ..Default::default()
        };

        let effect = apply_session_result_to_inner(
            &mut inner,
            7,
            SessionResultDecision::Success(SessionResultSuccessDecision {
                final_text: Some("final text".into()),
                status_message: Some("non-fatal warning".into()),
                hide_delay: Duration::from_millis(2500),
            }),
        );

        assert_eq!(
            effect,
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::HideOverlayAfter(Duration::from_millis(2500)),
                log_message: None,
            }
        );
        assert_eq!(inner.stage, SessionStage::Success);
        assert_eq!(inner.last_text.as_deref(), Some("final text"));
        assert_eq!(inner.status_message.as_deref(), Some("non-fatal warning"));
        assert!(inner.status_message_expires_at.is_some());
    }

    #[test]
    fn stale_success_result_leaves_inner_unchanged_atomically() {
        let mut inner = Inner {
            stage: SessionStage::Cancelled,
            session_id: 8,
            last_text: Some("newer text".into()),
            status_message: Some("newer warning".into()),
            ..Default::default()
        };

        let effect = apply_session_result_to_inner(
            &mut inner,
            7,
            SessionResultDecision::Success(SessionResultSuccessDecision {
                final_text: Some("late transcript".into()),
                status_message: Some("late warning".into()),
                hide_delay: Duration::from_millis(2500),
            }),
        );

        assert_eq!(
            effect,
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::Ignore,
                log_message: None,
            }
        );
        assert_eq!(inner.stage, SessionStage::Cancelled);
        assert_eq!(inner.session_id, 8);
        assert_eq!(inner.last_text.as_deref(), Some("newer text"));
        assert_eq!(inner.status_message.as_deref(), Some("newer warning"));
    }

    #[test]
    fn stale_error_result_produces_no_log_or_state_changes() {
        let mut inner = Inner {
            stage: SessionStage::Cancelled,
            session_id: 8,
            last_text: Some("newer text".into()),
            status_message: Some("newer warning".into()),
            ..Default::default()
        };

        let effect = apply_session_result_to_inner(
            &mut inner,
            7,
            SessionResultDecision::Error {
                status_message: "late error".into(),
            },
        );

        assert_eq!(
            effect,
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::Ignore,
                log_message: None,
            }
        );
        assert_eq!(inner.stage, SessionStage::Cancelled);
        assert_eq!(inner.session_id, 8);
        assert_eq!(inner.last_text.as_deref(), Some("newer text"));
        assert_eq!(inner.status_message.as_deref(), Some("newer warning"));
    }

    #[test]
    fn matching_failed_result_requests_log_after_atomic_apply() {
        let mut inner = Inner {
            stage: SessionStage::Transcribing,
            session_id: 7,
            ..Default::default()
        };

        let effect = apply_session_result_to_inner(
            &mut inner,
            7,
            SessionResultDecision::Failed {
                final_text: Some("recoverable text".into()),
                status_message: "Accessibility permissions required".into(),
            },
        );

        assert_eq!(
            effect,
            SessionResultApplyOutcome {
                side_effect: SessionResultSideEffect::ShowOverlay,
                log_message: Some(
                    "session failed stage=failed: Accessibility permissions required".into(),
                ),
            }
        );
        assert_eq!(inner.stage, SessionStage::Error);
        assert_eq!(inner.last_text.as_deref(), Some("recoverable text"));
        assert_eq!(
            inner.status_message.as_deref(),
            Some("Accessibility permissions required")
        );
    }

    #[test]
    fn cancel_recording_stage_requests_recorder_cancel_and_bumps_session() {
        let decision = resolve_cancel_decision(SessionStage::Recording, 41, true);

        assert_eq!(
            decision,
            CancelDecision::Cancel(CancelSessionDecision {
                next_session_id: 42,
                abort_processing_task: true,
                should_cancel_recorder: true,
            })
        );
    }

    #[test]
    fn cancel_processing_stage_skips_recorder_cancel_and_bumps_session() {
        let decision = resolve_cancel_decision(SessionStage::Transcribing, 41, true);

        assert_eq!(
            decision,
            CancelDecision::Cancel(CancelSessionDecision {
                next_session_id: 42,
                abort_processing_task: true,
                should_cancel_recorder: false,
            })
        );
    }

    #[test]
    fn cancel_idle_stage_reports_not_recording_without_bumping_session() {
        let decision = resolve_cancel_decision(SessionStage::Idle, 9, false);

        assert_eq!(decision, CancelDecision::NotRecording);
    }

    #[tokio::test]
    async fn cancel_processing_stage_aborts_task_and_bumps_session_id() {
        let controller = SessionController::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let dropped_flag = dropped.clone();
        let task = tauri::async_runtime::spawn(async move {
            let _guard = DropFlag(dropped_flag);
            std::future::pending::<()>().await;
        });

        {
            let mut inner = controller.inner.lock().await;
            inner.stage = SessionStage::Transcribing;
            inner.session_id = 41;
            inner.processing_task = Some(task);
        }

        let app = tauri::test::mock_app();
        let result = controller.cancel_recording(app.handle(), test_service()).await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        let inner = controller.inner.lock().await;
        assert_eq!(result.stage, "cancelled");
        assert_eq!(inner.stage, SessionStage::Cancelled);
        assert_eq!(inner.session_id, 42);
        assert!(inner.processing_task.is_none());
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancel_idle_stage_returns_not_recording_via_controller() {
        let controller = SessionController::new();
        {
            let mut inner = controller.inner.lock().await;
            inner.stage = SessionStage::Idle;
            inner.session_id = 9;
        }

        let app = tauri::test::mock_app();
        let result = controller.cancel_recording(app.handle(), test_service()).await;

        let inner = controller.inner.lock().await;
        assert_eq!(result.stage, "idle");
        assert_eq!(result.error.as_deref(), Some("not recording"));
        assert_eq!(inner.stage, SessionStage::Idle);
        assert_eq!(inner.session_id, 9);
    }

    #[tokio::test]
    async fn stale_success_result_does_not_override_cancelled_session_state() {
        let controller = SessionController::new();
        {
            let mut inner = controller.inner.lock().await;
            inner.stage = SessionStage::Cancelled;
            inner.session_id = 8;
            inner.last_text = None;
            inner.status_message = None;
            inner.status_message_expires_at = None;
        }

        let app = tauri::test::mock_app();
        let applied = controller
            .apply_session_result(
                app.handle(),
                7,
                SessionResultDecision::Success(SessionResultSuccessDecision {
                    final_text: Some("late transcript".into()),
                    status_message: Some("late warning".into()),
                    hide_delay: Duration::from_millis(2500),
                }),
            )
            .await;

        assert!(!applied);

        let inner = controller.inner.lock().await;
        assert_eq!(inner.stage, SessionStage::Cancelled);
        assert_eq!(inner.session_id, 8);
        assert_eq!(inner.last_text, None);
        assert_eq!(inner.status_message, None);
    }

    #[tokio::test]
    async fn cancel_holds_transition_and_concurrent_stop_reports_busy() {
        let controller = SessionController::new();
        {
            let mut inner = controller.inner.lock().await;
            inner.stage = SessionStage::Recording;
            inner.session_id = 17;
        }

        controller.arm_transition_pause();

        let app = tauri::test::mock_app();
        let cancel_app = app.handle().clone();
        let stop_app = app.handle().clone();
        let cancel_controller = controller.clone();

        let cancel_task = tauri::async_runtime::spawn(async move {
            cancel_controller
                .cancel_recording(&cancel_app, test_service())
                .await
        });

        tokio::time::timeout(
            Duration::from_secs(1),
            controller.wait_for_transition_pause(),
        )
        .await
        .unwrap();

        let stop_result = controller.toggle_recording(&stop_app, test_service()).await;
        controller.release_transition_pause();
        let cancel_result = cancel_task.await.unwrap();

        assert_eq!(cancel_result.stage, "cancelled");
        assert_eq!(stop_result.stage, "busy");
        assert_eq!(stop_result.error.as_deref(), Some("busy"));

        let inner = controller.inner.lock().await;
        assert_eq!(inner.stage, SessionStage::Cancelled);
        assert_eq!(inner.session_id, 18);
    }

    #[tokio::test]
    async fn stop_holds_transition_and_concurrent_cancel_reports_busy() {
        let controller = SessionController::new();
        {
            let mut inner = controller.inner.lock().await;
            inner.stage = SessionStage::Recording;
            inner.session_id = 21;
        }

        controller.arm_transition_pause();

        let app = tauri::test::mock_app();
        let stop_app = app.handle().clone();
        let cancel_app = app.handle().clone();
        let stop_controller = controller.clone();

        let stop_task = tauri::async_runtime::spawn(async move {
            stop_controller.toggle_recording(&stop_app, test_service()).await
        });

        tokio::time::timeout(
            Duration::from_secs(1),
            controller.wait_for_transition_pause(),
        )
        .await
        .unwrap();

        let cancel_result = controller.cancel_recording(&cancel_app, test_service()).await;
        controller.release_transition_pause();
        let stop_result = stop_task.await.unwrap();

        assert_eq!(cancel_result.stage, "busy");
        assert_eq!(cancel_result.error.as_deref(), Some("busy"));
        assert_ne!(stop_result.stage, "busy");

        let inner = controller.inner.lock().await;
        assert_eq!(inner.session_id, 21);
    }

}

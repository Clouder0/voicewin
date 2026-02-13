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

#[cfg(any(windows, target_os = "macos"))]
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

    pub async fn mark_overlay_ready(&self, app: &tauri::AppHandle) {
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

    pub async fn emit_status(&self, app: &tauri::AppHandle) {
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

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn emit_mic_level(&self, app: &tauri::AppHandle, rms: f32, peak: f32) {
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

    pub async fn set_stage(&self, app: &tauri::AppHandle, stage: SessionStage) {
        {
            let mut inner = self.inner.lock().await;

            let prev = inner.stage;

            // If we're leaving Recording, preserve the final elapsed time so the overlay timer
            // doesn't jump back to 0 immediately.
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
        self.emit_status(app).await;
    }

    #[allow(dead_code)]
    pub async fn set_last_text(&self, text: Option<String>) {
        let mut inner = self.inner.lock().await;
        inner.last_text = text;
    }

    fn show_overlay(app: &tauri::AppHandle) {
        if let Some(w) = app.get_webview_window("recording_overlay") {
            let _ = w.show();
        }
    }

    async fn show_overlay_and_sync(&self, app: &tauri::AppHandle) {
        // On some platforms a hidden webview may miss events; showing first and
        // then emitting status makes the overlay self-healing.
        Self::show_overlay(app);
        self.emit_status(app).await;
    }

    async fn hide_overlay_if_session_matches(
        &self,
        app: &tauri::AppHandle,
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

    async fn set_status_message(&self, app: &tauri::AppHandle, message: String, ttl: Duration) {
        {
            let mut inner = self.inner.lock().await;
            inner.status_message = Some(message);
            inner.status_message_expires_at = Some(Instant::now() + ttl);
        }
        self.emit_status(app).await;
    }

    #[allow(dead_code)]
    async fn mark_error(&self, app: &tauri::AppHandle, error: String) {
        log::error!("session error: {error}");
        self.set_stage(app, SessionStage::Error).await;
        self.set_status_message(app, error, Duration::from_secs(6))
            .await;

        // Always surface errors in the HUD.
        self.show_overlay_and_sync(app).await;
    }

    pub async fn cancel_recording(&self, app: &tauri::AppHandle, svc: AppService) -> ToggleResult {
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

        let stage = { self.inner.lock().await.stage };
        match stage {
            SessionStage::Recording => {
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
                if let Some(task) = self.inner.lock().await.processing_task.take() {
                    task.abort();
                }

                // Bump the session id so any pending work/hide from the previous session can't win.
                let session_id = {
                    let mut inner = self.inner.lock().await;
                    inner.session_id = inner.session_id.wrapping_add(1);
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
            SessionStage::Finalizing
            | SessionStage::Transcribing
            | SessionStage::Enhancing
            | SessionStage::Inserting => {
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
                    inner.session_id = inner.session_id.wrapping_add(1);
                    (inner.session_id, inner.processing_task.take())
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
            _ => {
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

    pub async fn toggle_recording(&self, app: &tauri::AppHandle, svc: AppService) -> ToggleResult {
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

                        fn merge_warning(dst: &mut Option<String>, msg: String) {
                            let msg = msg.trim().to_string();
                            if msg.is_empty() {
                                return;
                            }
                            *dst = match dst.take() {
                                Some(existing) if !existing.trim().is_empty() => {
                                    Some(format!("{existing} | {msg}"))
                                }
                                _ => Some(msg),
                            };
                        }

                        if let Some(rt) = realtime {
                            let finalize_budget = Duration::from_secs(2);
                            let finalize_started = Instant::now();

                            let mut allow_override = true;

                            let captured = rt.captured_frames.load(Ordering::Relaxed);
                            let sent = rt.sent_frames.load(Ordering::Relaxed);
                            let backlog_frames = captured.saturating_sub(sent);
                            log::info!(
                                "ElevenLabs realtime stop: captured_frames={captured} sent_frames={sent} backlog_frames={backlog_frames} (~{}ms)",
                                backlog_frames as u64 * REALTIME_FRAME_MS_DEFAULT
                            );

                            let dropped = rt.dropped_chunks.load(Ordering::Relaxed);
                            if dropped > 0 {
                                let msg = format!(
                                    "ElevenLabs realtime dropped {dropped} audio chunks; transcript may be incomplete."
                                );
                                log::warn!("{msg}");
                                merge_warning(&mut warning, msg.clone());
                                // Audio was lost client-side; don't trust realtime output.
                                allow_override = false;
                                controller
                                    .set_status_message(
                                        &app_handle,
                                        msg,
                                        Duration::from_millis(2500),
                                    )
                                    .await;
                            }

                            // Surface any provider-side warnings (e.g. outbound backpressure drops).
                            if let Ok(guard) = rt.last_warning.lock() {
                                if let Some(w) = guard.clone() {
                                    log::warn!("ElevenLabs realtime warning (recording): {w}");
                                    // If the provider reports drops/backpressure, don't trust realtime output.
                                    if w.to_lowercase().contains("dropped")
                                        || w.to_lowercase().contains("backpressure")
                                    {
                                        allow_override = false;
                                    }
                                    merge_warning(&mut warning, w);
                                }
                            }

                            // If the receiver recorded an error, treat realtime as unreliable.
                            if let Ok(guard) = rt.last_error.lock() {
                                if let Some(e) = guard.clone() {
                                    if !e.trim().is_empty() {
                                        allow_override = false;
                                    }
                                }
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
                                            merge_warning(
                                                &mut warning,
                                                "ElevenLabs realtime could not queue final audio frame; using batch on stop.".into(),
                                            );
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
                                        merge_warning(
                                            &mut warning,
                                            "ElevenLabs realtime uploader is not running; using batch on stop.".into(),
                                        );
                                    } else {
                                        let drain_start = Instant::now();
                                        match tokio::time::timeout(remaining, rx).await {
                                            Ok(Ok(ok)) if ok => {}
                                            Ok(Ok(_)) => {
                                                allow_override = false;
                                                merge_warning(
                                                    &mut warning,
                                                    "ElevenLabs realtime uploader failed while draining; using batch on stop.".into(),
                                                );
                                            }
                                            Ok(Err(_)) => {
                                                allow_override = false;
                                                merge_warning(
                                                    &mut warning,
                                                    "ElevenLabs realtime uploader drain cancelled; using batch on stop.".into(),
                                                );
                                            }
                                            Err(_) => {
                                                allow_override = false;
                                                merge_warning(
                                                    &mut warning,
                                                    "ElevenLabs realtime could not catch up in 2s; using batch on stop.".into(),
                                                );
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
                                            if let Some(t) =
                                                voicewin_core::stt::accept_transcript_override(t)
                                            {
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
                                                merge_warning(&mut warning, msg.clone());
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
                                            let msg = format!(
                                                "ElevenLabs realtime failed; using batch on stop. ({detail})"
                                            );
                                            log::warn!("{msg}");
                                            merge_warning(&mut warning, msg.clone());
                                            controller
                                                .set_status_message(
                                                    &app_handle,
                                                    msg,
                                                    Duration::from_millis(2500),
                                                )
                                                .await;
                                        }
                                        Err(_) => {
                                            let msg = "ElevenLabs realtime could not finalize in 2s; using batch on stop.".to_string();
                                            log::warn!("{msg}");
                                            merge_warning(&mut warning, msg.clone());
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
                                        // Map engine stage labels to overlay stages.
                                        match stage {
                                            "transcribing" => {
                                                let s = if using_override {
                                                    SessionStage::Finalizing
                                                } else {
                                                    SessionStage::Transcribing
                                                };
                                                controller_for_hook
                                                    .set_stage(&app_for_hook, s)
                                                    .await;
                                            }
                                            "enhancing" => {
                                                controller_for_hook
                                                    .set_stage(
                                                        &app_for_hook,
                                                        SessionStage::Enhancing,
                                                    )
                                                    .await;
                                            }
                                            "inserting" => {
                                                controller_for_hook
                                                    .set_stage(
                                                        &app_for_hook,
                                                        SessionStage::Inserting,
                                                    )
                                                    .await;
                                            }
                                            _ => {}
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

                        // Ignore late results from a cancelled/replaced session.
                        let still_current = {
                            let inner = controller.inner.lock().await;
                            inner.session_id == session_id
                        };
                        if !still_current {
                            return;
                        }

                        match res {
                            Ok(r) => {
                                controller.set_last_text(r.final_text.clone()).await;

                                if r.stage == "done" {
                                    // If we have a non-fatal warning (e.g. enhancement failed), show it briefly.
                                    let delay = if let Some(msg) =
                                        r.error.as_ref().filter(|s| !s.trim().is_empty())
                                    {
                                        controller
                                            .set_status_message(
                                                &app_handle,
                                                msg.clone(),
                                                Duration::from_millis(2500),
                                            )
                                            .await;
                                        Duration::from_millis(2500)
                                    } else {
                                        Self::OVERLAY_HIDE_DELAY
                                    };

                                    controller
                                        .set_stage(&app_handle, SessionStage::Success)
                                        .await;

                                    // After entering Recording, the session id was incremented in `set_stage`.
                                    let session_id = { controller.inner.lock().await.session_id };
                                    let controller2 = controller.clone();
                                    let app_handle2 = app_handle.clone();

                                    tauri::async_runtime::spawn(async move {
                                        controller2
                                            .hide_overlay_if_session_matches(
                                                &app_handle2,
                                                session_id,
                                                delay,
                                            )
                                            .await;
                                    });
                                } else if r.stage == "failed" {
                                    // Insertion failed but the text should be recoverable via History.
                                    controller.set_stage(&app_handle, SessionStage::Error).await;

                                    // Preserve the underlying error string so the overlay can provide
                                    // actionable shortcuts (e.g. Accessibility settings on macOS).
                                    let msg = r.error.clone().unwrap_or_else(|| {
                                        "Could not insert. Saved to History.".into()
                                    });

                                    log::error!("session failed stage=failed: {msg}");
                                    controller
                                        .set_status_message(
                                            &app_handle,
                                            msg,
                                            Duration::from_secs(6),
                                        )
                                        .await;
                                    Self::show_overlay(&app_handle);
                                } else {
                                    controller.set_stage(&app_handle, SessionStage::Error).await;
                                    Self::show_overlay(&app_handle);
                                }
                            }
                            Err(e) => {
                                controller.mark_error(&app_handle, e.to_string()).await;
                                Self::show_overlay(&app_handle);
                            }
                        }
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
}

use std::sync::Arc;
#[cfg(any(windows, target_os = "macos"))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(any(windows, target_os = "macos"))]
use std::sync::Mutex as StdMutex;
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
struct RealtimeSttState {
    handle: ElevenLabsRealtimeHandle,
    sender_task: tauri::async_runtime::JoinHandle<()>,
    receiver_task: tauri::async_runtime::JoinHandle<()>,
    streaming_enabled: Arc<AtomicBool>,
    dropped_chunks: Arc<AtomicU64>,

    // Best-effort diagnostics/warnings to surface on stop (and persist to History).
    last_error: Arc<StdMutex<Option<String>>>,
    last_warning: Arc<StdMutex<Option<String>>>,
}

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

#[derive(Clone, Default)]
pub struct SessionController {
    #[allow(dead_code)]
    inner: Arc<Mutex<Inner>>,
}

impl SessionController {
    const MAX_RECORDING_DURATION: Duration = Duration::from_secs(120);
    const BUSY_TOAST_TTL: Duration = Duration::from_secs(1);
    // Design-draft: Success state must remain visible for 1500ms before exit.
    const OVERLAY_HIDE_DELAY: Duration = Duration::from_millis(1500);

    pub fn new() -> Self {
        Self::default()
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
            inner.session_id == session_id && matches!(inner.stage, SessionStage::Success | SessionStage::Cancelled)
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
                        rt.sender_task.abort();
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
                        rt.sender_task.abort();
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
        // Minimal controller behavior:
        // - idle -> start recording
        // - recording -> stop and run
        // - busy -> ignore (for now)
        let stage = { self.inner.lock().await.stage };

        match stage {
            SessionStage::Idle | SessionStage::Error | SessionStage::Cancelled | SessionStage::Success => {
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
                    if let Ok(cfg) = svc.load_config() {
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
                        effective_language = Some(eff.language);
                    }

                    let eleven_key = if wants_realtime {
                        get_secret(SecretKey::ElevenLabsApiKey).ok().flatten().unwrap_or_default()
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
                    let streaming_enabled = Arc::new(AtomicBool::new(wants_realtime));
                    let dropped_chunks = Arc::new(AtomicU64::new(0));
                    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(64);

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
                            let audio_tx = audio_tx.clone();
                            move |chunk: &[f32]| {
                                let now = Instant::now();

                                // For realtime STT, do NOT throttle or drop chunks here.
                                // Send every chunk best-effort and let the bounded channel provide backpressure.
                                if streaming_enabled.load(Ordering::Relaxed) {
                                    if audio_tx.try_send(chunk.to_vec()).is_err() {
                                        dropped_chunks.fetch_add(1, Ordering::Relaxed);
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
                                guard.smoothed_rms = smooth_level(
                                    guard.smoothed_rms,
                                    rms,
                                    dt,
                                );
                                guard.smoothed_peak = smooth_level(
                                    guard.smoothed_peak,
                                    peak,
                                    dt,
                                );

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
                        let sr = svc
                            .recording_sample_rate_hz()
                            .await
                            .unwrap_or(16_000);

                        let mut rt_cfg = match ElevenLabsRealtimeConfig::production(eleven_key, sr) {
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

                        match spawn_realtime_session(rt_cfg).await {
                            Ok((handle, mut events)) => {
                                let last_error = Arc::new(StdMutex::new(None));
                                let last_warning = Arc::new(StdMutex::new(None));
                                let session_id_for_realtime = { controller.inner.lock().await.session_id };

                                // Sender task: convert f32 -> PCM16 and stream to WS.
                                let handle_for_sender = handle.clone();
                                let streaming_enabled_for_sender = streaming_enabled.clone();
                                let sender_task = tauri::async_runtime::spawn(async move {
                                    while let Some(chunk) = audio_rx.recv().await {
                                        if !streaming_enabled_for_sender.load(Ordering::Relaxed) {
                                            continue;
                                        }
                                        let pcm = pcm_s16le_from_f32(&chunk);
                                        if !handle_for_sender.send_audio_chunk(pcm).await {
                                            // Realtime session died; disable streaming so the audio callback stops enqueueing.
                                            streaming_enabled_for_sender.store(false, Ordering::Relaxed);
                                            break;
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
                                    while let Some(evt) = events.recv().await {
                                        // Don't let stale realtime updates leak into a cancelled/new session.
                                        if receiver_controller.inner.lock().await.session_id != session_id_for_realtime {
                                            break;
                                        }

                                        match evt {
                                            RealtimeEvent::SessionStarted { .. } => {}
                                            RealtimeEvent::LiveText { committed, partial } => {
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
                                                if last_emit.elapsed() < Duration::from_millis(200) {
                                                    continue;
                                                }
                                                last_emit = Instant::now();
                                                receiver_controller.set_last_text(Some(live)).await;
                                                receiver_controller.emit_status(&receiver_app).await;
                                            }
                                            RealtimeEvent::Warning { kind: _, message } => {
                                                // Persist the latest warning so stop-time History can reflect it.
                                                if let Ok(mut guard) = last_warning_for_receiver.lock() {
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
                                            RealtimeEvent::Error { message_type, error } => {
                                                // Stop feeding realtime immediately; we'll fall back to batch on stop.
                                                streaming_enabled_for_receiver.store(false, Ordering::Relaxed);

                                                // Store a concise detail for stop-time warnings.
                                                if let Ok(mut guard) = last_error_for_receiver.lock() {
                                                    *guard = Some(format!("{message_type}: {error}"));
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
                                        sender_task,
                                        receiver_task,
                                        streaming_enabled: streaming_enabled.clone(),
                                        dropped_chunks: dropped_chunks.clone(),
                                        last_error,
                                        last_warning,
                                    });
                                }
                            }
                            Err(e) => {
                                log::warn!("failed to start ElevenLabs realtime; will fall back to batch on stop: {e}");
                                streaming_enabled.store(false, Ordering::Relaxed);
                                controller
                                    .set_status_message(
                                        &app_handle,
                                        format!("ElevenLabs realtime unavailable; will use batch on stop. ({e})"),
                                        Duration::from_millis(2500),
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
                        // No more audio will be sent after stop; abort the sender task.
                        rt.sender_task.abort();
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
                            let dropped = rt.dropped_chunks.load(Ordering::Relaxed);
                            if dropped > 0 {
                                let msg = format!(
                                    "ElevenLabs realtime dropped {dropped} audio chunks; transcript may be incomplete."
                                );
                                merge_warning(&mut warning, msg.clone());
                                controller
                                    .set_status_message(&app_handle, msg, Duration::from_millis(2500))
                                    .await;
                            }

                            // Surface any provider-side warnings (e.g. outbound backpressure drops).
                            if let Ok(guard) = rt.last_warning.lock() {
                                if let Some(w) = guard.clone() {
                                    merge_warning(&mut warning, w);
                                }
                            }

                            match rt.handle.finalize().await {
                                Ok(t) => {
                                    if let Some(t) = voicewin_core::stt::accept_transcript_override(t) {
                                        transcript_override = t;
                                    } else {
                                        let msg = "ElevenLabs realtime produced no text; using batch on stop.".to_string();
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
                                Err(e) => {
                                    let detail = rt
                                        .last_error
                                        .lock()
                                        .ok()
                                        .and_then(|g| g.clone())
                                        .unwrap_or_else(|| e.to_string());
                                    let msg = format!(
                                        "ElevenLabs realtime failed; using batch on stop. ({detail})"
                                    );
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

                            rt.receiver_task.abort();
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
                                                controller_for_hook.set_stage(&app_for_hook, s).await;
                                            }
                                            "enhancing" => {
                                                controller_for_hook
                                                    .set_stage(&app_for_hook, SessionStage::Enhancing)
                                                    .await;
                                            }
                                            "inserting" => {
                                                controller_for_hook
                                                    .set_stage(&app_for_hook, SessionStage::Inserting)
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
                                    let delay = if let Some(msg) = r.error.as_ref().filter(|s| !s.trim().is_empty()) {
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

                                    controller.set_stage(&app_handle, SessionStage::Success).await;

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
                                    let msg = r
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "Could not insert. Saved to History.".into());

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

// No unit tests here: this file is a Tauri implementation detail and these helpers are
// only used when the recording path is enabled on the current OS.

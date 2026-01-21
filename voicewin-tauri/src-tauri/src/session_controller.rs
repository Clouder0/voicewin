use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use voicewin_appcore::service::AppService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStage {
    Idle,
    Recording,
    Transcribing,
    Enhancing,
    Inserting,
    Done,
    Error,
    Cancelled,
}

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
    pub last_text_preview: Option<String>,
    pub last_text_available: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MicLevelPayload {
    pub rms: f32,
    pub peak: f32,
}

#[derive(Debug, Default)]
struct Inner {
    stage: SessionStage,
    recording_started_at: Option<Instant>,
    recording_elapsed_ms: Option<u64>,
    last_text: Option<String>,
    status_message: Option<String>,
    status_message_expires_at: Option<Instant>,
    session_id: u64,
}

#[derive(Clone, Default)]
pub struct SessionController {
    inner: Arc<Mutex<Inner>>,
}

impl SessionController {
    const MAX_RECORDING_DURATION: Duration = Duration::from_secs(120);
    const BUSY_TOAST_TTL: Duration = Duration::from_secs(1);
    const OVERLAY_HIDE_DELAY: Duration = Duration::from_millis(450);

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
            last_text_available: inner.last_text.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
        }
    }

    pub async fn emit_status(&self, app: &tauri::AppHandle) {
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

        let payload = SessionStatusPayload {
            stage: inner.stage,
            stage_label: stage_label(inner.stage).into(),
            is_recording: inner.stage == SessionStage::Recording,
            elapsed_ms,
            error: inner.status_message.clone(),
            last_text_preview,
            last_text_available: inner.last_text.as_ref().map(|t| !t.is_empty()).unwrap_or(false),
        };

        let _ = app.emit(crate::EVENT_SESSION_STATUS, payload);
    }

    pub async fn emit_mic_level(&self, app: &tauri::AppHandle, rms: f32, peak: f32) {
        let _ = app.emit(
            crate::EVENT_MIC_LEVEL,
            MicLevelPayload {
                rms: rms.clamp(0.0, 1.0),
                peak: peak.clamp(0.0, 1.0),
            },
        );
    }

    pub async fn set_stage(&self, app: &tauri::AppHandle, stage: SessionStage) {
        {
            let mut inner = self.inner.lock().await;

            // If we're leaving Recording, preserve the final elapsed time so the overlay timer
            // doesn't jump back to 0 immediately.
            if inner.stage == SessionStage::Recording && stage != SessionStage::Recording {
                if let Some(start) = inner.recording_started_at {
                    inner.recording_elapsed_ms = Some(start.elapsed().as_millis() as u64);
                }
                inner.recording_started_at = None;
            }

            inner.stage = stage;

            if stage == SessionStage::Recording {
                inner.session_id = inner.session_id.wrapping_add(1);
                inner.recording_started_at = Some(Instant::now());
                inner.recording_elapsed_ms = None;
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

    pub async fn set_last_text(&self, text: Option<String>) {
        let mut inner = self.inner.lock().await;
        inner.last_text = text;
    }

    fn show_overlay(app: &tauri::AppHandle) {
        if let Some(w) = app.get_webview_window("recording_overlay") {
            let _ = w.show();
        }
    }

    async fn hide_overlay_if_session_matches(&self, app: &tauri::AppHandle, session_id: u64) {
        // Delay a bit so the user can see the completed stage.
        tokio::time::sleep(Self::OVERLAY_HIDE_DELAY).await;

        let should_hide = {
            let inner = self.inner.lock().await;
            inner.session_id == session_id && inner.stage != SessionStage::Recording
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

    async fn mark_error(&self, app: &tauri::AppHandle, error: String) {
        self.set_stage(app, SessionStage::Error).await;
        self.set_status_message(app, error, Duration::from_secs(6)).await;
    }

    pub async fn cancel_recording(&self, app: &tauri::AppHandle, svc: AppService) -> ToggleResult {
        let stage = { self.inner.lock().await.stage };
        if stage != SessionStage::Recording {
            self.set_status_message(app, "not recording".into(), Self::BUSY_TOAST_TTL)
                .await;
            return ToggleResult {
                stage: "idle".into(),
                final_text: None,
                error: Some("not recording".into()),
                is_recording: false,
            };
        }

        #[cfg(windows)]
        {
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

        #[cfg(not(windows))]
        {
            let _ = svc;
        }

        self.set_stage(app, SessionStage::Cancelled).await;
        Self::show_overlay(app);

        // Bump the session id so any pending hide from the previous session can't win.
        let session_id = {
            let mut inner = self.inner.lock().await;
            inner.session_id = inner.session_id.wrapping_add(1);
            inner.session_id
        };
        {
            let controller = self.clone();
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                controller
                    .hide_overlay_if_session_matches(&app_handle, session_id)
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

    pub async fn toggle_recording(&self, app: &tauri::AppHandle, svc: AppService) -> ToggleResult {
        // Minimal controller behavior:
        // - idle -> start recording
        // - recording -> stop and run
        // - busy -> ignore (for now)
        let stage = { self.inner.lock().await.stage };

        match stage {
            SessionStage::Idle | SessionStage::Done | SessionStage::Error | SessionStage::Cancelled => {
                self.set_stage(app, SessionStage::Recording).await;
                Self::show_overlay(app);

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

                #[cfg(windows)]
                {
                    let controller = self.clone();
                    let app_handle = app.clone();

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
                            move |chunk: &[f32]| {
                                let now = Instant::now();

                                let mut guard = match level_state.lock() {
                                    Ok(g) => g,
                                    Err(poisoned) => poisoned.into_inner(),
                                };

                                let dt = now.duration_since(guard.last_emit);
                                if dt < Duration::from_millis(50) {
                                    return;
                                }
                                guard.last_emit = now;

                                let (rms, peak) = crate::session_controller::compute_levels(chunk);
                                guard.smoothed_rms = crate::session_controller::smooth_level(
                                    guard.smoothed_rms,
                                    rms,
                                    dt,
                                );
                                guard.smoothed_peak = crate::session_controller::smooth_level(
                                    guard.smoothed_peak,
                                    peak,
                                    dt,
                                );

                                let (rms_out, peak_out) = (guard.smoothed_rms, guard.smoothed_peak);
                                drop(guard);

                                let controller = controller.clone();
                                let app_handle = app_handle.clone();
                                tauri::async_runtime::spawn(async move {
                                    controller
                                        .emit_mic_level(&app_handle, rms_out, peak_out)
                                        .await;
                                });
                            }
                        })
                        .await
                    {
                        controller.mark_error(&app_handle, e.to_string()).await;
                        return ToggleResult {
                            stage: "error".into(),
                            final_text: None,
                            error: Some(e.to_string()),
                            is_recording: false,
                        };
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
                self.set_stage(app, SessionStage::Transcribing).await;

                if let Some(w) = app.get_webview_window("recording_overlay") {
                    let _ = w.show();
                }

                #[cfg(windows)]
                {
                    let audio = match svc.clone().stop_recording().await {
                        Ok(a) => a,
                        Err(e) => {
                            self.mark_error(app, e.to_string()).await;
                            return ToggleResult {
                                stage: "error".into(),
                                final_text: None,
                                error: Some(e.to_string()),
                                is_recording: false,
                            };
                        }
                    };

                    // Run engine pipeline.
                    let res = svc
                        .clone()
                        .run_session(
                            voicewin_runtime::ipc::RunSessionRequest {
                                transcript: String::new(),
                            },
                            audio,
                        )
                        .await;

                    match res {
                        Ok(r) => {
                            self.set_last_text(r.final_text.clone()).await;
                            self.set_stage(app, SessionStage::Done).await;

                // After entering Recording, the session id was incremented in `set_stage`.
                let session_id = { self.inner.lock().await.session_id };
                            {
                                let controller = self.clone();
                                let app_handle = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    controller
                                        .hide_overlay_if_session_matches(&app_handle, session_id)
                                        .await;
                                });
                            }

                            ToggleResult {
                                stage: r.stage,
                                final_text: r.final_text,
                                error: r.error,
                                is_recording: false,
                            }
                        }
                        Err(e) => {
                            self.mark_error(app, e.to_string()).await;
                            Self::show_overlay(app);

                            ToggleResult {
                                stage: "error".into(),
                                final_text: None,
                                error: Some(e.to_string()),
                                is_recording: false,
                            }
                        }
                    }
                }

                #[cfg(not(windows))]
                {
                    let _ = svc;
                    self.set_stage(app, SessionStage::Error).await;
                    self.set_status_message(app, "recording only supported on Windows".into(), Duration::from_secs(3))
                        .await;
                    ToggleResult {
                        stage: "error".into(),
                        final_text: None,
                        error: Some("recording only supported on Windows".into()),
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
        SessionStage::Transcribing => "transcribing",
        SessionStage::Enhancing => "enhancing",
        SessionStage::Inserting => "inserting",
        SessionStage::Done => "done",
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

pub fn smooth_level(prev: f32, next: f32, dt: Duration) -> f32 {
    // Exponential smoothing with a 150ms time constant.
    let tau = 0.15f32;
    let alpha = 1.0 - (-dt.as_secs_f32() / tau).exp();
    prev + (next - prev) * alpha
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_zero_for_empty() {
        assert_eq!(compute_levels(&[]), (0.0, 0.0));
    }

    #[test]
    fn levels_detect_peak_and_rms() {
        let (rms, peak) = compute_levels(&[0.0, 1.0, -1.0, 0.0]);
        assert!((peak - 1.0).abs() < 1e-6);
        assert!(rms > 0.0);
    }

    #[test]
    fn smooth_moves_towards_target() {
        let out = smooth_level(0.0, 1.0, Duration::from_millis(50));
        assert!(out > 0.0);
        assert!(out < 1.0);
    }
}

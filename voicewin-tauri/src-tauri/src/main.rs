#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

// Tracks whether the user is currently dragging the overlay. We only persist overlay
// move events while this flag is set to avoid persisting on normal clicks or programmatic moves.
static OVERLAY_IS_DRAGGING: std::sync::OnceLock<std::sync::atomic::AtomicBool> =
    std::sync::OnceLock::new();

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OverlayMovedPayload {
    x: i32,
    y: i32,
}

#[cfg(any(windows, target_os = "macos"))]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(windows)]
use window_vibrancy::{apply_acrylic, apply_tabbed};

#[cfg(target_os = "linux")]
fn load_tray_icon(app: &tauri::AppHandle) -> Option<tauri::image::Image<'static>> {
    let path = app
        .path()
        .resolve("icons/32x32.png", tauri::path::BaseDirectory::Resource)
        .ok()?;

    tauri::image::Image::from_path(path)
        .ok()
        .map(|i| i.to_owned())
}

#[cfg(not(target_os = "linux"))]
fn load_tray_icon(_app: &tauri::AppHandle) -> Option<tauri::image::Image<'static>> {
    None
}
use voicewin_appcore::service::AppService;
use voicewin_core::config::AppConfig;


#[derive(Debug, Clone, serde::Serialize)]
struct DownloadProgress {
    model_id: String,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ModelCatalogEntry {
    id: String,
    title: String,
    recommended: bool,
    filename: String,
    size_bytes: Option<u64>,
    speed_label: Option<String>,
    accuracy_label: Option<String>,

    installed: bool,
    active: bool,
    downloading: bool,
}

// In-memory download state so Model Library can reflect "Downloading".
static DOWNLOADING_MODELS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

const EVENT_MODEL_DOWNLOAD_PROGRESS: &str = "voicewin://model_download_progress";
const EVENT_MODEL_DOWNLOAD_DONE: &str = "voicewin://model_download_done";

#[cfg(any(windows, target_os = "macos"))]
use voicewin_audio::AudioRecorder;

mod session_controller;
use session_controller::{SessionController, ToggleResult};

// Design-draft: pill bottom should be 80px above the monitor bottom.
const OVERLAY_BOTTOM_OFFSET: i32 = 80;

const OVERLAY_POSITION_STORE_PATH: &str = "ui_state.json";
const OVERLAY_POSITION_STORE_KEY: &str = "overlay_position";

#[cfg(any(windows, target_os = "macos"))]
const HOTKEY_STORE_KEY: &str = "toggle_hotkey";

#[cfg(windows)]
const DEFAULT_TOGGLE_HOTKEY: &str = "Ctrl+Space";

#[cfg(target_os = "macos")]
const DEFAULT_TOGGLE_HOTKEY: &str = "Alt+Z";

pub const EVENT_SESSION_STATUS: &str = "voicewin://session_status";
#[cfg(any(windows, target_os = "macos"))]
pub const EVENT_MIC_LEVEL: &str = "voicewin://mic_level";
pub const EVENT_TOGGLE_HOTKEY_CHANGED: &str = "voicewin://toggle_hotkey_changed";

struct AppState {
    service: tokio::sync::OnceCell<AppService>,
    session: SessionController,

    #[cfg(any(windows, target_os = "macos"))]
    toggle_hotkey: std::sync::Mutex<String>,
}

fn default_config_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    Ok(dir.join("config.json"))
}

fn default_history_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    Ok(dir.join("history.json"))
}

fn ensure_bootstrap_model(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let app_data_dir = app.path().app_data_dir()?;

    let dst = voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir);
    if dst.exists() {
        // If the file is present but invalid (partial/corrupt), re-copy from bundled resources.
        if voicewin_runtime::models::validate_bootstrap_model(&dst).is_ok() {
            return Ok(dst);
        }
    }

    let src = app
        .path()
        .resolve(
            "models/bootstrap.gguf",
            tauri::path::BaseDirectory::Resource,
        )
        .or_else(|_| {
            // Fallback: some bundlers keep resources flat.
            app.path().resolve(
                "resources/models/bootstrap.gguf",
                tauri::path::BaseDirectory::Resource,
            )
        })?;

    voicewin_runtime::models::atomic_copy(&src, &dst)?;

    // Fail fast if the bundled model is missing/corrupt.
    voicewin_runtime::models::validate_bootstrap_model(&dst)?;

    Ok(dst)
}

async fn build_service(app: &tauri::AppHandle) -> anyhow::Result<AppService> {
    let config_path = default_config_path(app)?;

    // Ensure the bundled bootstrap model is available on disk.
    // The bootstrap model is required for out-of-box local STT.
    let _ = ensure_bootstrap_model(app)?;

    // Platform providers
    #[cfg(windows)]
    let ctx: Arc<dyn voicewin_engine::traits::AppContextProvider> =
        Arc::new(voicewin_platform::windows::WindowsContextProvider::default());
    #[cfg(target_os = "macos")]
    let ctx: Arc<dyn voicewin_engine::traits::AppContextProvider> =
        Arc::new(voicewin_platform::macos::MacosContextProvider::default());
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let ctx: Arc<dyn voicewin_engine::traits::AppContextProvider> =
        voicewin_platform::test::TestContextProvider::new(
            voicewin_core::types::AppIdentity::new().with_process_name("linux"),
            Default::default(),
        )
        .boxed();

    #[cfg(windows)]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::windows::WindowsInserter::default());
    #[cfg(target_os = "macos")]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::macos::MacosInserter::default());
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::test::StdoutInserter);

    Ok(AppService::new(config_path, ctx, inserter))
}

fn init_default_config(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    let mut d = voicewin_runtime::defaults::default_global_defaults();

    // Prefer the user-installed large-v3-turbo "preferred" model if present.
    // Otherwise, fall back to the bundled bootstrap model.
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let preferred = voicewin_runtime::models::choose_default_local_stt_model_path(&app_data_dir);

    if preferred == voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir) {
        let model_path = ensure_bootstrap_model(app).map_err(|e| e.to_string())?;
        d.stt_model = model_path.to_string_lossy().to_string();
    } else {
        d.stt_model = preferred.to_string_lossy().to_string();
    }

    let cfg = voicewin_core::config::AppConfig {
        defaults: d,
        profiles: vec![],
        prompts: voicewin_runtime::defaults::default_prompt_templates(),
        llm_api_key_present: svc.get_openai_api_key_present().unwrap_or(false),
    };

    svc.save_config(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

fn load_or_init_config(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    match svc.load_config() {
        Ok(cfg) => Ok(cfg),
        Err(_) => init_default_config(svc, app),
    }
}



fn normalize_model_path_to_models_dir(
    app_data_dir: &std::path::Path,
    path: &str,
) -> Option<String> {
    // If the model path points anywhere under our models dir, normalize to the canonical filename.
    // This makes configs resilient to different path separators / user-selected filenames.
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return None;
    }

    let models_dir = voicewin_runtime::models::models_dir(app_data_dir);
    if let Ok(rel) = p.strip_prefix(&models_dir) {
        if rel == std::path::Path::new(voicewin_runtime::models::PREFERRED_LOCAL_STT_MODEL_FILENAME)
        {
            return Some(
                voicewin_runtime::models::installed_preferred_local_stt_model_path(app_data_dir)
                    .to_string_lossy()
                    .to_string(),
            );
        }
        if rel == std::path::Path::new(voicewin_runtime::models::BOOTSTRAP_MODEL_FILENAME) {
            return Some(
                voicewin_runtime::models::installed_bootstrap_model_path(app_data_dir)
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }

    None
}

fn validate_config(cfg: &AppConfig) -> Result<(), String> {
    if cfg.defaults.stt_provider == "local" {
        // For local whisper, `stt_model` must be a path to a GGUF file.
        let p = std::path::Path::new(&cfg.defaults.stt_model);
        if !p.exists() {
            return Err(format!(
                "local STT model does not exist: {}",
                cfg.defaults.stt_model
            ));
        }
        voicewin_runtime::models::validate_gguf_file(p, 1024 * 1024).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<AppConfig, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    load_or_init_config(svc, &app)
}

#[tauri::command]
async fn set_config(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    mut cfg: AppConfig,
) -> Result<(), String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    // Normalize known model filenames in our app models dir.
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if let Some(normalized) =
        normalize_model_path_to_models_dir(&app_data_dir, &cfg.defaults.stt_model)
    {
        cfg.defaults.stt_model = normalized;
    }

    validate_config(&cfg)?;

    svc.save_config(&cfg).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ForegroundAppInfo {
    process_name: Option<String>,
    exe_path: Option<String>,
    window_title: Option<String>,
}

#[tauri::command]
async fn capture_foreground_app(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<ForegroundAppInfo, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let app_id = svc
        .get_foreground_app()
        .await
        .map_err(|e| e.to_string())?;

    Ok(ForegroundAppInfo {
        process_name: app_id.process_name.map(|p| p.0),
        exe_path: app_id.exe_path.map(|p| p.0),
        window_title: app_id.window_title.map(|t| t.0),
    })
}

#[tauri::command]
async fn cancel_recording(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ToggleResult, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(state.session.cancel_recording(&app, svc.clone()).await)
}

#[tauri::command]
async fn toggle_recording(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ToggleResult, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(state.session.toggle_recording(&app, svc.clone()).await)
}

#[cfg(any(windows, target_os = "macos"))]
#[derive(serde::Serialize)]
struct HotkeyState {
    hotkey: String,
    error: Option<String>,
}

#[cfg(any(windows, target_os = "macos"))]
fn current_hotkey(state: &State<'_, AppState>) -> String {
    state
        .toggle_hotkey
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

#[cfg(any(windows, target_os = "macos"))]
fn set_hotkey_in_state(state: &State<'_, AppState>, value: String) {
    let mut guard = state
        .toggle_hotkey
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    *guard = value;
}

#[cfg(any(windows, target_os = "macos"))]
#[tauri::command]
async fn get_toggle_hotkey(state: State<'_, AppState>) -> Result<HotkeyState, String> {
    Ok(HotkeyState {
        hotkey: current_hotkey(&state),
        error: None,
    })
}

#[cfg(any(windows, target_os = "macos"))]
#[tauri::command]
async fn set_toggle_hotkey(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    hotkey: String,
) -> Result<HotkeyState, String> {
    let prev = current_hotkey(&state);

    // No-op if unchanged.
    if prev == hotkey {
        return Ok(HotkeyState {
            hotkey,
            error: None,
        });
    }

    // Best-effort: unregister previous hotkey.
    let _ = app.global_shortcut().unregister(prev.as_str());

    // Try registering the new hotkey.
    let res = app.global_shortcut().on_shortcut(hotkey.as_str(), {
        let session = state.session.clone();
        let svc_cell = state.service.clone();
        move |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }

            let app = app.clone();
            let session = session.clone();
            let svc_cell = svc_cell.clone();

            tauri::async_runtime::spawn(async move {
                let svc = match svc_cell
                    .get_or_try_init(|| async { build_service(&app).await })
                    .await
                {
                    Ok(s) => s,
                    Err(_) => return,
                };

                let _ = session.toggle_recording(&app, svc.clone()).await;
            });
        }
    });

    if let Err(e) = res {
        // Restore previous hotkey registration (best-effort).
        let _ = app.global_shortcut().on_shortcut(prev.as_str(), {
            let session = state.session.clone();
            let svc_cell = state.service.clone();
            move |app, _shortcut, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }

                let app = app.clone();
                let session = session.clone();
                let svc_cell = svc_cell.clone();

                tauri::async_runtime::spawn(async move {
                    let svc = match svc_cell
                        .get_or_try_init(|| async { build_service(&app).await })
                        .await
                    {
                        Ok(s) => s,
                        Err(_) => return,
                    };

                    let _ = session.toggle_recording(&app, svc.clone()).await;
                });
            }
        });

        return Ok(HotkeyState {
            hotkey: prev,
            error: Some(format!("failed to register hotkey: {e}")),
        });
    }

    set_hotkey_in_state(&state, hotkey.clone());

    if let Ok(store) = app.store(OVERLAY_POSITION_STORE_PATH) {
        store.set(HOTKEY_STORE_KEY, serde_json::Value::String(hotkey.clone()));
        let _ = store.save();
    }

    let _ = app.emit(EVENT_TOGGLE_HOTKEY_CHANGED, hotkey.clone());

    Ok(HotkeyState {
        hotkey,
        error: None,
    })
}



#[tauri::command]
async fn get_history(
    app: tauri::AppHandle,
) -> Result<Vec<voicewin_runtime::history::HistoryEntry>, String> {
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    store.load().map_err(|e| e.to_string())
}


#[tauri::command]
async fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    store.clear().map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_history_entry(app: tauri::AppHandle, ts_unix_ms: i64, text: String) -> Result<bool, String> {
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    store
        .delete_entry(ts_unix_ms, &text)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ModelStatus {
    pub bootstrap_ok: bool,
    pub bootstrap_path: String,
    pub preferred_ok: bool,
    pub preferred_path: String,
}

#[cfg(any(windows, target_os = "macos"))]
#[tauri::command]
async fn list_microphones() -> Result<Vec<String>, String> {
    AudioRecorder::list_input_device_names().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_model_status(app: tauri::AppHandle) -> Result<ModelStatus, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let bootstrap_path = voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir);
    let preferred_path =
        voicewin_runtime::models::installed_preferred_local_stt_model_path(&app_data_dir);

    let bootstrap_ok = voicewin_runtime::models::validate_bootstrap_model(&bootstrap_path).is_ok();
    let preferred_ok =
        voicewin_runtime::models::validate_gguf_file(&preferred_path, 50 * 1024 * 1024).is_ok();

    Ok(ModelStatus {
        bootstrap_ok,
        bootstrap_path: bootstrap_path.to_string_lossy().to_string(),
        preferred_ok,
        preferred_path: preferred_path.to_string_lossy().to_string(),
    })
}



#[tauri::command]
async fn list_models(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<ModelCatalogEntry>, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let cfg = load_or_init_config(svc, &app)?;

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let models_dir = voicewin_runtime::models::models_dir(&app_data_dir);

    let active_path = std::path::PathBuf::from(cfg.defaults.stt_model);

    let mut out = Vec::new();
    for spec in voicewin_runtime::models::whisper_catalog() {
        let path = models_dir.join(&spec.filename);
        let installed = path.exists();
        let active = installed && active_path == path;

        let downloading = DOWNLOADING_MODELS
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
            .lock()
            .ok()
            .map(|g| g.contains(&spec.id))
            .unwrap_or(false);

        out.push(ModelCatalogEntry {
            id: spec.id,
            title: spec.title,
            recommended: spec.recommended,
            filename: spec.filename,
            size_bytes: spec.size_bytes,
            speed_label: spec.speed_label,
            accuracy_label: spec.accuracy_label,
            installed,
            active,
            downloading,
        });
    }

    Ok(out)
}

#[tauri::command]
async fn set_active_model(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    model_id: String,
) -> Result<(), String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let mut cfg = load_or_init_config(svc, &app)?;

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let models_dir = voicewin_runtime::models::models_dir(&app_data_dir);

    let spec = voicewin_runtime::models::whisper_catalog()
        .into_iter()
        .find(|s| s.id == model_id)
        .ok_or_else(|| "unknown model id".to_string())?;

    let path = models_dir.join(&spec.filename);
    if !path.exists() {
        return Err("model not installed".into());
    }

    cfg.defaults.stt_provider = "local".into();
    cfg.defaults.stt_model = path.to_string_lossy().to_string();

    validate_config(&cfg)?;
    svc.save_config(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
async fn download_model(app: tauri::AppHandle, model_id: String) -> Result<(), String> {
    // NOTE: this uses network access (HuggingFace).
    let downloading = DOWNLOADING_MODELS.get_or_init(|| {
        std::sync::Mutex::new(std::collections::HashSet::new())
    });

    {
        let mut guard = downloading.lock().map_err(|_| "download lock poisoned".to_string())?;
        if guard.contains(&model_id) {
            return Err("model is already downloading".into());
        }
        guard.insert(model_id.clone());
    }

    let result = async {
        let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let models_dir = voicewin_runtime::models::models_dir(&app_data_dir);
        voicewin_runtime::models::ensure_dir(&models_dir).map_err(|e| e.to_string())?;

        let spec = voicewin_runtime::models::whisper_catalog()
            .into_iter()
            .find(|s| s.id == model_id)
            .ok_or_else(|| "unknown model id".to_string())?;

        let dst = models_dir.join(&spec.filename);
        let expected_sha = spec.sha256.to_lowercase();

        // Stream download into a temp file.
        let tmp = dst.with_extension("download");
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }

        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;

        let req = reqwest::Client::new().get(&spec.url);
        let resp = req.send().await.map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if !(200..=299).contains(&status) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("download failed: status={status}"));
        }

        let total = resp.content_length();
        let mut stream = resp.bytes_stream();

        use futures_util::StreamExt;
        use sha2::Digest;

        let mut hasher = sha2::Sha256::new();
        let mut downloaded: u64 = 0;
        let mut last_emit = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e.to_string());
                }
            };

            downloaded += chunk.len() as u64;
            hasher.update(&chunk);

            if let Err(e) = std::io::Write::write_all(&mut f, &chunk) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.to_string());
            }

            // Throttle progress events to avoid spamming the UI.
            if last_emit.elapsed() >= std::time::Duration::from_millis(120) {
                last_emit = std::time::Instant::now();
                let _ = app.emit(
                    EVENT_MODEL_DOWNLOAD_PROGRESS,
                    DownloadProgress {
                        model_id: model_id.clone(),
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                    },
                );
            }
        }

        // Final progress emit.
        let _ = app.emit(
            EVENT_MODEL_DOWNLOAD_PROGRESS,
            DownloadProgress {
                model_id: model_id.clone(),
                downloaded_bytes: downloaded,
                total_bytes: total,
            },
        );

        f.sync_all().ok();

        let got_sha = format!("{:x}", hasher.finalize());
        if got_sha != expected_sha {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "checksum mismatch (expected {expected_sha}, got {got_sha})"
            ));
        }

        // Basic sanity (GGUF magic + non-trivial size).
        if let Err(e) = voicewin_runtime::models::validate_gguf_file(&tmp, 10 * 1024 * 1024) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }

        // Replace into final destination.
        voicewin_runtime::models::replace_file(&tmp, &dst).map_err(|e| e.to_string())?;

        let _ = app.emit(EVENT_MODEL_DOWNLOAD_DONE, model_id.clone());
        Ok(())
    }
    .await;

    // Clear downloading state.
    let _ = downloading
        .lock()
        .map(|mut g| {
            g.remove(&model_id);
        })
        .map_err(|_| "download lock poisoned".to_string());

    result
}

#[tauri::command]
async fn overlay_drag_begin(_app: tauri::AppHandle) -> Result<(), String> {
    // Mark that subsequent window moved events are user-driven.
    let flag = OVERLAY_IS_DRAGGING.get_or_init(|| std::sync::atomic::AtomicBool::new(false));
    flag.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn overlay_drag_end(app: tauri::AppHandle) -> Result<(), String> {
    let flag = OVERLAY_IS_DRAGGING.get_or_init(|| std::sync::atomic::AtomicBool::new(false));
    flag.store(false, std::sync::atomic::Ordering::SeqCst);

    // Persist current position at the end of the drag.
    if let Some(w) = app.get_webview_window("recording_overlay") {
        if let Ok(pos) = w.outer_position() {
            if let Ok(store) = app.store(OVERLAY_POSITION_STORE_PATH) {
                let payload = OverlayMovedPayload { x: pos.x, y: pos.y };
                if let Ok(v) = serde_json::to_value(&payload) {
                    store.set(OVERLAY_POSITION_STORE_KEY, v);
                    let _ = store.save();
                }
            }
        }
    }

    Ok(())
}

#[tauri::command]
async fn overlay_set_size(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("recording_overlay") {
        // JS measures in CSS pixels (logical units), so resize in logical units.
        let _ = w.set_size(tauri::Size::Logical(tauri::LogicalSize::new(width, height)));

        // If the user has not dragged the overlay (no stored position), keep it centered after
        // fit-content resizes so it doesn't drift.
        let has_saved_position = app
            .store(OVERLAY_POSITION_STORE_PATH)
            .ok()
            .and_then(|s| s.get(OVERLAY_POSITION_STORE_KEY))
            .is_some();

        if !has_saved_position {
            if let Ok(Some(monitor)) = w.current_monitor().or_else(|_| w.primary_monitor()) {
                let work = monitor.work_area();
                if let Ok(size) = w.outer_size() {
                    let x = work.position.x + (work.size.width as i32 / 2) - (size.width as i32 / 2);

                    // Place the pill so its bottom is 80px above the monitor bottom.
                    // (We align the window bottom accordingly; the webview itself includes shadow padding.)
                    let y = work.position.y + work.size.height as i32 - OVERLAY_BOTTOM_OFFSET - (size.height as i32);

                    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn overlay_dismiss(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("recording_overlay") {
        let _ = w.hide();
    }
    Ok(())
}

#[tauri::command]
async fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn open_macos_accessibility_settings() -> Result<(), String> {
    use std::process::Command;

    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    let status = Command::new("open").arg(url).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("failed to open Accessibility settings".into())
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn open_macos_microphone_settings() -> Result<(), String> {
    use std::process::Command;

    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
    let status = Command::new("open").arg(url).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("failed to open Microphone settings".into())
    }
}

fn main() {
    // Persist logs to the OS log dir so Windows users can debug issues even in
    // `windows_subsystem = "windows"` builds (no console output).
    use tauri_plugin_log::{Target, TargetKind};

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([Target::new(TargetKind::LogDir {
                    file_name: Some("voicewin".into()),
                })])
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // If a second instance is launched, bring the existing window to the front.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(AppState {
            service: tokio::sync::OnceCell::new(),
            session: SessionController::new(),

            #[cfg(any(windows, target_os = "macos"))]
            toggle_hotkey: std::sync::Mutex::new(DEFAULT_TOGGLE_HOTKEY.into()),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            toggle_recording,
            cancel_recording,
            #[cfg(any(windows, target_os = "macos"))]
            get_toggle_hotkey,
            #[cfg(any(windows, target_os = "macos"))]
            set_toggle_hotkey,

            get_history,
            clear_history,
            delete_history_entry,
            get_model_status,
            #[cfg(any(windows, target_os = "macos"))]
            list_microphones,
            list_models,
            download_model,
            set_active_model,
            capture_foreground_app,
            overlay_drag_begin,
            overlay_drag_end,
            overlay_set_size,
            overlay_dismiss,
            show_main_window,

            #[cfg(target_os = "macos")]
            open_macos_accessibility_settings,
            #[cfg(target_os = "macos")]
            open_macos_microphone_settings,
        ])
        .setup(|app| {
            let handle = app.handle();

            // Overlay window (hidden by default). This is the primary UX feedback surface.
            // Default size is only used until the webview measures the HUD pill.
            let overlay = WebviewWindowBuilder::new(
                handle,
                "recording_overlay",
                WebviewUrl::App("src/overlay.html".into()),
            )
            .title("VoiceWin")
            .visible(false)
            .focusable(false)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .transparent(true)
            .shadow(false)
            .inner_size(240.0, 72.0)
            .build()?;

            // Apply Acrylic to overlay (best-effort; Windows-only).
            #[cfg(windows)]
            {
                let _ = apply_acrylic(&overlay, Some((0, 0, 0, 0)));
            }

            // Apply Mica Alt (tabbed) to the main window (best-effort; Windows-only).
            #[cfg(windows)]
            {
                if let Some(main_w) = app.get_webview_window("main") {
                    let _ = apply_tabbed(&main_w, None);
                }
            }


            // IMPORTANT: do not set the overlay window as click-through by default.
            // The HUD contains interactive controls (Stop/Cancel/History/Dismiss) and must
            // receive pointer events.

            // If the user previously moved the overlay, restore that position.
            // Otherwise, center on the current monitor (or primary) and move it near the bottom.
            let mut restored = false;
            if let Ok(store) = app.store(OVERLAY_POSITION_STORE_PATH) {
                if let Some(v) = store.get(OVERLAY_POSITION_STORE_KEY) {
                    if let Ok(p) = serde_json::from_value::<OverlayMovedPayload>(v) {
                        // Validate against the available monitor work areas.
                        if let Ok(monitors) = overlay.available_monitors() {
                            let fits_any = monitors.iter().any(|m| {
                                let work = m.work_area();
                                let left = work.position.x;
                                let top = work.position.y;
                                let right = work.position.x + work.size.width as i32;
                                let bottom = work.position.y + work.size.height as i32;

                                // Conservative bounds: ensure the overlay top-left is on-screen.
                                // The overlay is resized dynamically after the webview measures content.
                                p.x >= left && p.x <= right && p.y >= top && p.y <= bottom
                            });

                            if fits_any {
                                let _ = overlay.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition::new(p.x, p.y),
                                ));
                                restored = true;
                            }
                        }
                    }
                }
            }

            if !restored {
                // Center on the current monitor (or primary), then move it near the bottom.
                if let Ok(Some(monitor)) = overlay
                    .current_monitor()
                    .or_else(|_| overlay.primary_monitor())
                {
                    let work = monitor.work_area();
                    let size = &work.size;
                    let pos = &work.position;

                    if let Ok(size_px) = overlay.outer_size() {
                        let x = pos.x + (size.width as i32 / 2) - (size_px.width as i32 / 2);

                        // Align the overlay window bottom so the pill appears ~80px above the monitor bottom.
                        let y = pos.y + size.height as i32
                            - OVERLAY_BOTTOM_OFFSET
                            - (size_px.height as i32);

                        let _ = overlay.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x, y),
                        ));
                    }

                    // Overlay must remain interactive; do not enable click-through.
                }
            }

            // Persist overlay position only while user is actively dragging.
            // This avoids accidentally persisting position on normal clicks or programmatic moves.
            let store_for_events = app.store(OVERLAY_POSITION_STORE_PATH).ok();
            overlay.on_window_event({
                let store_for_events = store_for_events.clone();
                move |event| {
                    use tauri::WindowEvent;
                    if !matches!(event, WindowEvent::Moved(_)) {
                        return;
                    }

                    let flag = OVERLAY_IS_DRAGGING
                        .get_or_init(|| std::sync::atomic::AtomicBool::new(false))
                        .load(std::sync::atomic::Ordering::SeqCst);
                    if !flag {
                        return;
                    }

                    let WindowEvent::Moved(pos) = event else { return; };

                    if let Some(store) = store_for_events.as_ref() {
                        let payload = OverlayMovedPayload { x: pos.x, y: pos.y };
                        if let Ok(v) = serde_json::to_value(&payload) {
                            store.set(OVERLAY_POSITION_STORE_KEY, v);
                            let _ = store.save();
                        }
                    }
                }
            });

            // Store for later menu events.
            let _overlay = overlay;

            let show_main = MenuItemBuilder::new("Show").id("show").build(handle)?;
            let toggle = MenuItemBuilder::new("Start Recording")
                .id("toggle_recording")
                .build(handle)?;
            let cancel = MenuItemBuilder::new("Cancel Recording")
                .id("cancel_recording")
                .build(handle)?;
            let open_history = MenuItemBuilder::new("Open History")
                .id("open_history")
                .build(handle)?;
            let reset_hud_position = MenuItemBuilder::new("Reset HUD Position")
                .id("reset_hud_position")
                .build(handle)?;
            let quit = MenuItemBuilder::new("Quit").id("quit").build(handle)?;

            let menu = MenuBuilder::new(handle)
                .items(&[
                    &show_main,
                    &toggle,
                    &cancel,
                    &open_history,
                    &reset_hud_position,
                    &quit,
                ])
                .build()?;

            let mut tray_builder = TrayIconBuilder::with_id("tray").menu(&menu);
            if let Some(icon) = load_tray_icon(handle) {
                tray_builder = tray_builder.icon(icon);
            }

            let app_state = app.state::<AppState>();
            let session = app_state.session.clone();

            let tray = tray_builder
                .on_menu_event({
                    let session = session.clone();
                    move |app, event| match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "toggle_recording" => {
                            let app = app.clone();
                            let session = session.clone();
                            let state = app.state::<AppState>();
                            let svc_cell = state.service.clone();

                            // We update the label by holding onto the MenuItem handle.
                            let toggle_item = toggle.clone();

                            tauri::async_runtime::spawn(async move {
                                let svc = match svc_cell
                                    .get_or_try_init(|| async { build_service(&app).await })
                                    .await
                                {
                                    Ok(s) => s,
                                    Err(_) => return,
                                };

                                let res = session.toggle_recording(&app, svc.clone()).await;

                                let _ = toggle_item.set_text(if res.is_recording {
                                    "Stop Recording"
                                } else {
                                    "Start Recording"
                                });
                            });
                        }
                        "cancel_recording" => {
                            let app = app.clone();
                            let session = session.clone();
                            let state = app.state::<AppState>();
                            let svc_cell = state.service.clone();

                            tauri::async_runtime::spawn(async move {
                                let svc = match svc_cell
                                    .get_or_try_init(|| async { build_service(&app).await })
                                    .await
                                {
                                    Ok(s) => s,
                                    Err(_) => return,
                                };

                                let _ = session.cancel_recording(&app, svc.clone()).await;
                            });
                        }
                        "open_history" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                                // Best-effort: switch to the history tab.
                                let _ = w.emit("voicewin://navigate", "history");
                            }
                        }
                        "reset_hud_position" => {
                            if let Ok(store) = app.store(OVERLAY_POSITION_STORE_PATH) {
                                store.delete(OVERLAY_POSITION_STORE_KEY);
                                let _ = store.save();
                            }

                            if let Some(overlay) = app.get_webview_window("recording_overlay") {
                                if let Ok(Some(monitor)) = overlay
                                    .current_monitor()
                                    .or_else(|_| overlay.primary_monitor())
                                {
                                    let work = monitor.work_area();

                                    if let Ok(size) = overlay.outer_size() {
                                        let x = work.position.x
                                            + (work.size.width as i32 / 2)
                                            - (size.width as i32 / 2);

                                        // Align the overlay window bottom so the pill appears ~80px above the
                                        // monitor bottom (the window itself includes shadow padding).
                                        let y = work.position.y
                                            + work.size.height as i32
                                            - OVERLAY_BOTTOM_OFFSET
                                            - (size.height as i32);

                                        let _ = overlay.set_position(tauri::Position::Physical(
                                            tauri::PhysicalPosition::new(x, y),
                                        ));
                                    }
                                }
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(handle)?;

            #[cfg(any(windows, target_os = "macos"))]
            {
                // Register the persisted (or default) toggle hotkey.
                // If registration fails (conflict), we keep running without a hotkey until the
                // user changes it from the UI.
                let handle = handle.clone();
                let app_handle = handle.clone();

                // Load persisted hotkey from store.
                let persisted = app
                    .store(OVERLAY_POSITION_STORE_PATH)
                    .ok()
                    .and_then(|s| s.get(HOTKEY_STORE_KEY))
                    .and_then(|v| v.as_str().map(|s| s.to_string()));

                let hotkey = persisted.unwrap_or_else(|| DEFAULT_TOGGLE_HOTKEY.into());

                // Keep in state for UI to query.
                if let Ok(mut guard) = app_state.toggle_hotkey.lock() {
                    *guard = hotkey.clone();
                } else {
                    *app_state
                        .toggle_hotkey
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = hotkey.clone();
                }

                // Register with handler.
                let session = session.clone();
                let svc_cell = app_state.service.clone();

                let _ = app_handle.global_shortcut().on_shortcut(
                    hotkey.as_str(),
                    move |app, _shortcut, event| {
                        if event.state != ShortcutState::Pressed {
                            return;
                        }

                        let app = app.clone();
                        let session = session.clone();
                        let svc_cell = svc_cell.clone();

                        tauri::async_runtime::spawn(async move {
                            let svc = match svc_cell
                                .get_or_try_init(|| async { build_service(&app).await })
                                .await
                            {
                                Ok(s) => s,
                                Err(_) => return,
                            };

                            let _ = session.toggle_recording(&app, svc.clone()).await;
                        });
                    },
                );
            }

            let _ = tray;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

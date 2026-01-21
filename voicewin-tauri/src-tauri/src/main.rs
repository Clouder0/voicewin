#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Listener, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OverlayMovedPayload {
    x: i32,
    y: i32,
}

#[cfg(windows)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(target_os = "linux")]
fn load_tray_icon(app: &tauri::AppHandle) -> Option<tauri::image::Image<'static>> {
    let path = app
        .path()
        .resolve("icons/32x32.png", tauri::path::BaseDirectory::Resource)
        .ok()?;

    tauri::image::Image::from_path(path).ok().map(|i| i.to_owned())
}

#[cfg(not(target_os = "linux"))]
fn load_tray_icon(_app: &tauri::AppHandle) -> Option<tauri::image::Image<'static>> {
    None
}
use voicewin_appcore::service::AppService;
use voicewin_core::config::AppConfig;
use voicewin_runtime::ipc::{RunSessionRequest, RunSessionResponse};

mod session_controller;
use session_controller::{SessionController, ToggleResult};

const OVERLAY_WIDTH: f64 = 420.0;
const OVERLAY_HEIGHT: f64 = 84.0;
const OVERLAY_MARGIN_BOTTOM: f64 = 36.0;

const OVERLAY_POSITION_STORE_PATH: &str = "ui_state.json";
const OVERLAY_POSITION_STORE_KEY: &str = "overlay_position";

pub const EVENT_SESSION_STATUS: &str = "voicewin://session_status";
pub const EVENT_MIC_LEVEL: &str = "voicewin://mic_level";

#[derive(Default)]
struct AppState {
    service: tokio::sync::OnceCell<AppService>,
    session: SessionController,
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
    #[cfg(not(windows))]
    let ctx: Arc<dyn voicewin_engine::traits::AppContextProvider> = voicewin_platform::test::TestContextProvider::new(
        voicewin_core::types::AppIdentity::new().with_process_name("linux"),
        Default::default(),
    )
    .boxed();

    #[cfg(windows)]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::windows::WindowsInserter::default());
    #[cfg(not(windows))]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> = Arc::new(voicewin_platform::test::StdoutInserter);

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

#[tauri::command]
async fn get_config(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<AppConfig, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    load_or_init_config(svc, &app)
}

fn normalize_model_path_to_models_dir(app_data_dir: &std::path::Path, path: &str) -> Option<String> {
    // If the model path points anywhere under our models dir, normalize to the canonical filename.
    // This makes configs resilient to different path separators / user-selected filenames.
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return None;
    }

    let models_dir = voicewin_runtime::models::models_dir(app_data_dir);
    if let Ok(rel) = p.strip_prefix(&models_dir) {
        if rel == std::path::Path::new(voicewin_runtime::models::PREFERRED_LOCAL_STT_MODEL_FILENAME) {
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
            return Err(format!("local STT model does not exist: {}", cfg.defaults.stt_model));
        }
        voicewin_runtime::models::validate_gguf_file(p, 1024 * 1024).map_err(|e| e.to_string())?;
    }

    Ok(())
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
    if let Some(normalized) = normalize_model_path_to_models_dir(&app_data_dir, &cfg.defaults.stt_model) {
        cfg.defaults.stt_model = normalized;
    }

    validate_config(&cfg)?;

    svc.save_config(&cfg).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ProviderStatus {
    openai_api_key_present: bool,
    elevenlabs_api_key_present: bool,
}

#[tauri::command]
async fn get_provider_status(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(ProviderStatus {
        openai_api_key_present: svc.get_openai_api_key_present().map_err(|e| e.to_string())?,
        elevenlabs_api_key_present: svc
            .get_elevenlabs_api_key_present()
            .map_err(|e| e.to_string())?,
    })
}

#[tauri::command]
async fn set_openai_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<(), String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.set_openai_api_key(&api_key).map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_elevenlabs_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<(), String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.set_elevenlabs_api_key(&api_key).map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_session(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    transcript: String,
) -> Result<RunSessionResponse, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    // MVP fallback: until recording UX is complete, this uses a short silence buffer.
    let audio = voicewin_engine::traits::AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 160],
    };

    svc.run_session(RunSessionRequest { transcript }, audio)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_recording(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<ToggleResult, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(state.session.cancel_recording(&app, svc.clone()).await)
}

#[tauri::command]
async fn toggle_recording(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<ToggleResult, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(state.session.toggle_recording(&app, svc.clone()).await)
}

#[tauri::command]
async fn get_session_status(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<session_controller::SessionStatusPayload, String> {
    let _ = app;
    Ok(state.session.get_status().await)
}

#[tauri::command]
async fn get_history(app: tauri::AppHandle) -> Result<Vec<voicewin_runtime::history::HistoryEntry>, String> {
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

#[derive(serde::Serialize)]
struct ModelStatus {
    pub bootstrap_ok: bool,
    pub bootstrap_path: String,
    pub preferred_ok: bool,
    pub preferred_path: String,
}

#[tauri::command]
async fn get_model_status(app: tauri::AppHandle) -> Result<ModelStatus, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let bootstrap_path = voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir);
    let preferred_path = voicewin_runtime::models::installed_preferred_local_stt_model_path(&app_data_dir);

    let bootstrap_ok = voicewin_runtime::models::validate_bootstrap_model(&bootstrap_path).is_ok();
    let preferred_ok = voicewin_runtime::models::validate_gguf_file(&preferred_path, 50 * 1024 * 1024).is_ok();

    Ok(ModelStatus {
        bootstrap_ok,
        bootstrap_path: bootstrap_path.to_string_lossy().to_string(),
        preferred_ok,
        preferred_path: preferred_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
async fn install_preferred_model(app: tauri::AppHandle, src_path: String) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let src = std::path::PathBuf::from(src_path);
    if !src.exists() {
        return Err(format!("model file does not exist: {}", src.display()));
    }

    // Basic validation before copying.
    voicewin_runtime::models::validate_gguf_file(&src, 50 * 1024 * 1024).map_err(|e| e.to_string())?;

    let dst = voicewin_runtime::models::installed_preferred_local_stt_model_path(&app_data_dir);
    voicewin_runtime::models::atomic_copy(&src, &dst).map_err(|e| e.to_string())?;

    // Validate the copied file.
    voicewin_runtime::models::validate_gguf_file(&dst, 50 * 1024 * 1024).map_err(|e| e.to_string())?;

    // Ensure config exists, then auto-switch to the preferred model.
    if let Ok(svc) = build_service(&app).await {
        if let Ok(mut cfg) = load_or_init_config(&svc, &app) {
            cfg.defaults.stt_model = dst.to_string_lossy().to_string();
            let _ = svc.save_config(&cfg);
        }
    }

    Ok(dst.to_string_lossy().to_string())
}

#[tauri::command]
async fn overlay_drag_begin(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("recording_overlay") {
        let _ = w.set_ignore_cursor_events(false);
    }
    Ok(())
}

#[tauri::command]
async fn overlay_drag_end(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("recording_overlay") {
        let _ = w.set_ignore_cursor_events(true);
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            service: tokio::sync::OnceCell::new(),
            session: SessionController::new(),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_provider_status,
            set_openai_api_key,
            set_elevenlabs_api_key,
            run_session,
            toggle_recording,
            cancel_recording,
            get_session_status,
            get_history,
            clear_history,
            get_model_status,
            install_preferred_model,
            overlay_drag_begin,
            overlay_drag_end,
        ])
        .setup(|app| {
            let handle = app.handle();

            // Overlay window (hidden by default). This is the primary UX feedback surface.
            // Place bottom-center with a small bottom margin.
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
            .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
            .build()?;

            let _ = overlay.set_ignore_cursor_events(true);

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

                                p.x >= left
                                    && p.y >= top
                                    && (p.x + OVERLAY_WIDTH as i32) <= right
                                    && (p.y + OVERLAY_HEIGHT as i32) <= bottom
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

                    let x = pos.x as f64 + (size.width as f64 - OVERLAY_WIDTH) / 2.0;
                    let y = pos.y as f64 + (size.height as f64 - OVERLAY_HEIGHT - OVERLAY_MARGIN_BOTTOM);

                    let _ = overlay.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                        x,
                        y,
                    )));

                    // Windows: keep the overlay in the click-through state after positioning.
                    let _ = overlay.set_ignore_cursor_events(true);
                }
            }

            // Persist overlay position when the user drags it.
            let overlay_for_events = overlay.clone();
            let store_for_events = app.store(OVERLAY_POSITION_STORE_PATH).ok();
            let _ = app.listen_any("voicewin://overlay_moved", move |e| {
                let payload_str = e.payload();
                if payload_str.is_empty() {
                    return;
                }

                let payload: Option<OverlayMovedPayload> = serde_json::from_str(payload_str).ok();
                let Some(p) = payload else {
                    return;
                };

                if let Some(store) = store_for_events.as_ref() {
                    if let Ok(v) = serde_json::to_value(&p) {
                        store.set(OVERLAY_POSITION_STORE_KEY, v);
                        let _ = store.save();
                    }
                }

                let _ = overlay_for_events.set_ignore_cursor_events(false);
                let _ = overlay_for_events.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                    p.x,
                    p.y,
                )));
                let _ = overlay_for_events.set_ignore_cursor_events(true);
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
            let quit = MenuItemBuilder::new("Quit").id("quit").build(handle)?;

            let menu = MenuBuilder::new(handle)
                .items(&[&show_main, &toggle, &cancel, &open_history, &quit])
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
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(handle)?;

            #[cfg(windows)]
            {
                // Register a global shortcut to toggle recording.
                let handle = handle.clone();
                let session = session.clone();
                let svc_cell = app_state.service.clone();

                handle
                    .global_shortcut()
                    .on_shortcut("Ctrl+Shift+Space", move |app, _shortcut, event| {
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
                    })
                    .ok();
            }

            let _ = tray;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

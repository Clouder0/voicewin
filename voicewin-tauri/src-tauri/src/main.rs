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
#[cfg(target_os = "macos")]
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_store::StoreExt;

const BUILD_GIT_SHA: &str = match option_env!("VOICEWIN_GIT_SHA") {
    Some(sha) => sha,
    None => "unknown",
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OverlayMovedPayload {
    x: i32,
    y: i32,
}

#[cfg(any(windows, target_os = "macos"))]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[cfg(windows)]
use window_vibrancy::apply_tabbed;

fn load_tray_icon(app: &tauri::AppHandle) -> Option<tauri::image::Image<'static>> {
    if let Some(icon) = tray_icon::load_embedded_tray_icon() {
        return Some(icon);
    }

    let path = match app
        .path()
        .resolve("icons/32x32.png", tauri::path::BaseDirectory::Resource)
    {
        Ok(path) => path,
        Err(e) => {
            log::error!("failed to resolve tray icon resource path: {e}");
            return None;
        }
    };

    match tauri::image::Image::from_path(path) {
        Ok(icon) => Some(icon.to_owned()),
        Err(e) => {
            log::error!("failed to decode tray icon resource file: {e}");
            None
        }
    }
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
static DOWNLOADING_MODELS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = std::sync::OnceLock::new();

const EVENT_MODEL_DOWNLOAD_PROGRESS: &str = "voicewin://model_download_progress";
const EVENT_MODEL_DOWNLOAD_DONE: &str = "voicewin://model_download_done";

const BUNDLED_TINY_MODEL_ID: &str = "whisper-tiny-bundled";

#[cfg(any(windows, target_os = "macos"))]
use voicewin_audio::{AudioInputDeviceInfo, AudioRecorder};

mod runtime_smoke;
mod session_controller;
mod startup_smoke;
mod tray_icon;
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
#[cfg(any(windows, target_os = "macos", test))]
pub const EVENT_MIC_LEVEL: &str = "voicewin://mic_level";
pub const EVENT_TOGGLE_HOTKEY_CHANGED: &str = "voicewin://toggle_hotkey_changed";

struct AppState {
    // IMPORTANT: `tokio::sync::OnceCell` implements `Clone` by creating a NEW cell.
    // We must wrap it in an `Arc` so all hotkey/tray callbacks share the same service
    // instance (and thus the same audio recorder state).
    service: Arc<tokio::sync::OnceCell<AppService>>,
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
    log::info!("bootstrap model dst: {}", dst.display());
    if dst.exists() {
        // If the file is present but invalid (partial/corrupt), re-copy from bundled resources.
        if voicewin_runtime::models::validate_bootstrap_model(&dst).is_ok() {
            log::info!("bootstrap model already present + valid");
            return Ok(dst);
        }

        log::warn!("bootstrap model present but invalid; will re-copy");
    }

    // Locate the bundled bootstrap model.
    // Note: `resolve` does not check existence, so we must probe candidate paths.
    let mut tried = Vec::new();
    let mut src: Option<PathBuf> = None;

    for rel in [
        // Most common layout: resources include the file under `models/`.
        "models/bootstrap.bin",
        // If the bundler preserved the `resources/` prefix.
        "resources/models/bootstrap.bin",
    ] {
        if let Ok(p) = app
            .path()
            .resolve(rel, tauri::path::BaseDirectory::Resource)
        {
            tried.push(p.clone());
            if p.exists() {
                src = Some(p);
                break;
            }
        }
    }

    // Extra fallbacks for portable/no-bundle layouts (adjacent to the executable).
    if src.is_none() {
        for rel in ["models/bootstrap.bin", "resources/models/bootstrap.bin"] {
            if let Ok(p) = app
                .path()
                .resolve(rel, tauri::path::BaseDirectory::Executable)
            {
                tried.push(p.clone());
                if p.exists() {
                    src = Some(p);
                    break;
                }
            }
        }
    }

    // If none of the resolved candidates exist, report a detailed error.
    let src = src.ok_or_else(|| {
        let tried = tried
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::anyhow!("could not locate bundled bootstrap model; tried: {tried}")
    })?;

    log::info!("bootstrap model src: {}", src.display());

    voicewin_runtime::models::atomic_copy(&src, &dst)?;

    log::info!("bootstrap model copied to {}", dst.display());

    // Fail fast if the bundled model is missing/corrupt.
    voicewin_runtime::models::validate_bootstrap_model(&dst)?;

    Ok(dst)
}

async fn build_service_base(app: &tauri::AppHandle) -> anyhow::Result<AppService> {
    let config_path = default_config_path(app)?;
    log::info!("build_service config_path: {}", config_path.display());

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
        Arc::new(voicewin_platform::linux::LinuxContextProvider::default());

    #[cfg(windows)]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::windows::WindowsInserter::default());
    #[cfg(target_os = "macos")]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::macos::MacosInserter::default());
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let inserter: Arc<dyn voicewin_engine::traits::Inserter> =
        Arc::new(voicewin_platform::linux::LinuxInserter::default());

    #[cfg(all(not(windows), not(target_os = "macos")))]
    log::info!(
        "linux platform provider enabled: clipboard_context=true clipboard_insert=true selected_text=false window_context=false screenshot_capture=false foreground_app_lookup=false"
    );

    Ok(AppService::new(config_path, ctx, inserter))
}

async fn build_service(app: &tauri::AppHandle) -> anyhow::Result<AppService> {
    let svc = build_service_base(app).await?;

    // Tray/hotkey flows can start sessions without ever opening the main UI.
    // Ensure config exists (and is valid) during service initialization so
    // `run_session_with_hook` never fails due to a missing config file.
    let mut cfg = load_or_init_config(&svc, app).map_err(anyhow::Error::msg)?;

    // If the config is invalid (most commonly: a stale GGUF path), do a targeted migration.
    if let Err(e) = validate_config(&cfg) {
        log::warn!("config invalid; attempting auto-migration: {e}");

        let changed = migrate_local_stt_model_path(&mut cfg, app).map_err(anyhow::Error::msg)?;
        if changed {
            svc.save_config(&cfg)
                .map_err(|e| anyhow::Error::msg(e.to_string()))?;
        }

        // Validate again; if it still fails, surface the error instead of wiping settings.
        if let Err(e) = validate_config(&cfg) {
            log::error!("config invalid after migration: {e}");
            return Err(anyhow::Error::msg(e));
        }
    }

    log::info!("build_service done");
    Ok(svc)
}

fn default_config_for_app(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    let mut d = voicewin_runtime::defaults::default_global_defaults();

    // Prefer the user-installed "preferred" model if present.
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
        llm_api_key_present: llm_api_key_present(svc),
    };

    Ok(cfg)
}

fn init_default_config(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    let cfg = default_config_for_app(svc, app)?;
    svc.save_config(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

fn runtime_smoke_config(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    let mut smoke_defaults = voicewin_runtime::defaults::default_global_defaults();
    let bootstrap_model_path = ensure_bootstrap_model(app).map_err(|e| e.to_string())?;
    smoke_defaults.stt_model = bootstrap_model_path.to_string_lossy().to_string();

    Ok(runtime_smoke::deterministic_runtime_smoke_config(
        AppConfig {
            defaults: smoke_defaults.clone(),
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: llm_api_key_present(svc),
        },
        smoke_defaults,
    ))
}

async fn build_runtime_smoke_service(
    app: &tauri::AppHandle,
) -> anyhow::Result<(AppService, AppConfig)> {
    let svc = build_service_base(app).await?;
    let cfg = runtime_smoke_config(&svc, app).map_err(anyhow::Error::msg)?;
    Ok((svc, cfg))
}

fn load_or_init_config(svc: &AppService, app: &tauri::AppHandle) -> Result<AppConfig, String> {
    match svc.load_config() {
        Ok(mut cfg) => {
            let mut changed = false;
            if voicewin_runtime::defaults::backfill_default_prompts(&mut cfg) {
                changed = true;
            }
            if voicewin_runtime::defaults::migrate_legacy_openai_defaults_to_recommended(&mut cfg)
            {
                changed = true;
            }
            if voicewin_runtime::defaults::migrate_legacy_openai_profile_overrides_to_recommended(
                &mut cfg,
            ) {
                changed = true;
            }
            if changed {
                svc.save_config(&cfg).map_err(|e| e.to_string())?;
            }
            Ok(cfg)
        }
        Err(_) => init_default_config(svc, app),
    }
}

fn migrate_local_stt_model_path(
    cfg: &mut AppConfig,
    app: &tauri::AppHandle,
) -> Result<bool, String> {
    if cfg.defaults.stt_provider != "local" {
        return Ok(false);
    }

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // If the preferred model is present and looks valid, pick it.
    let preferred =
        voicewin_runtime::models::installed_preferred_local_stt_model_path(&app_data_dir);
    if preferred.exists()
        && voicewin_runtime::models::validate_ggml_file(&preferred, 1024 * 1024).is_ok()
    {
        let next = preferred.to_string_lossy().to_string();
        if cfg.defaults.stt_model != next {
            cfg.defaults.stt_model = next;
            return Ok(true);
        }
        return Ok(false);
    }

    // Fall back to the bundled bootstrap model.
    let bootstrap = ensure_bootstrap_model(app).map_err(|e| e.to_string())?;
    let next = bootstrap.to_string_lossy().to_string();
    if cfg.defaults.stt_model != next {
        cfg.defaults.stt_model = next;
        return Ok(true);
    }

    Ok(false)
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

fn llm_api_key_present(svc: &AppService) -> bool {
    svc.get_openai_api_key_present().unwrap_or(false)
        || svc.get_gemini_api_key_present().unwrap_or(false)
}

fn validate_llm_provider_kind(value: &str, label: &str) -> Result<(), String> {
    match value.trim() {
        "openai_compatible" | "gemini" => Ok(()),
        other => Err(format!(
            "{label} must be one of: openai_compatible, gemini (got {other:?})"
        )),
    }
}

fn validate_llm_api_kind(value: &str, label: &str) -> Result<(), String> {
    match value.trim() {
        "responses_sse" | "chat_completions" | "stream_generate_content_sse" => Ok(()),
        other => Err(format!(
            "{label} must be one of: responses_sse, chat_completions, stream_generate_content_sse (got {other:?})"
        )),
    }
}

fn validate_llm_preflight_mode(value: &str, label: &str) -> Result<(), String> {
    match value.trim() {
        "off" | "http_connect" => Ok(()),
        other => Err(format!(
            "{label} must be one of: off, http_connect (got {other:?})"
        )),
    }
}

fn validate_llm_preflight_delay_ms(value: u64, label: &str) -> Result<(), String> {
    if value > 60_000 {
        return Err(format!(
            "{label} must be between 0 and 60000 milliseconds (got {value})"
        ));
    }

    Ok(())
}

fn validate_llm_reasoning_effort(value: Option<&str>, label: &str) -> Result<(), String> {
    match value.map(str::trim) {
        None | Some("") => Ok(()),
        Some("minimal" | "low" | "medium" | "high") => Ok(()),
        Some(other) => Err(format!(
            "{label} must be empty or one of: minimal, low, medium, high (got {other:?})"
        )),
    }
}

fn validate_llm_provider_api_pair(
    provider_kind: &str,
    api_kind: &str,
    label: &str,
) -> Result<(), String> {
    let provider_kind = provider_kind.trim();
    let api_kind = api_kind.trim();

    match provider_kind {
        "openai_compatible" if matches!(api_kind, "responses_sse" | "chat_completions") => Ok(()),
        "gemini" if api_kind == "stream_generate_content_sse" => Ok(()),
        "openai_compatible" => Err(format!(
            "{label} uses openai_compatible but api kind {api_kind:?} is not supported"
        )),
        "gemini" => Err(format!(
            "{label} uses gemini but api kind {api_kind:?} is not supported"
        )),
        _ => Ok(()),
    }
}

fn validate_config(cfg: &AppConfig) -> Result<(), String> {
    validate_llm_provider_kind(
        &cfg.defaults.llm_provider_kind,
        "defaults.llm_provider_kind",
    )?;
    validate_llm_api_kind(&cfg.defaults.llm_api_kind, "defaults.llm_api_kind")?;
    validate_llm_preflight_mode(
        &cfg.defaults.llm_preflight_mode,
        "defaults.llm_preflight_mode",
    )?;
    validate_llm_preflight_delay_ms(
        cfg.defaults.llm_preflight_delay_ms,
        "defaults.llm_preflight_delay_ms",
    )?;
    validate_llm_reasoning_effort(
        cfg.defaults.llm_reasoning_effort.as_deref(),
        "defaults.llm_reasoning_effort",
    )?;
    validate_llm_provider_api_pair(
        &cfg.defaults.llm_provider_kind,
        &cfg.defaults.llm_api_kind,
        "defaults",
    )?;

    for profile in &cfg.profiles {
        let profile_provider_kind = profile
            .overrides
            .llm_provider_kind
            .as_deref()
            .unwrap_or(&cfg.defaults.llm_provider_kind);
        let profile_api_kind = profile
            .overrides
            .llm_api_kind
            .as_deref()
            .unwrap_or(&cfg.defaults.llm_api_kind);

        if let Some(value) = profile.overrides.llm_provider_kind.as_deref() {
            let label = format!("profile '{}' override llm_provider_kind", profile.name);
            validate_llm_provider_kind(value, &label)?;
        }
        if let Some(value) = profile.overrides.llm_api_kind.as_deref() {
            let label = format!("profile '{}' override llm_api_kind", profile.name);
            validate_llm_api_kind(value, &label)?;
        }
        if let Some(value) = profile.overrides.llm_preflight_mode.as_deref() {
            let label = format!("profile '{}' override llm_preflight_mode", profile.name);
            validate_llm_preflight_mode(value, &label)?;
        }
        if let Some(value) = profile.overrides.llm_reasoning_effort.as_deref() {
            let label = format!("profile '{}' override llm_reasoning_effort", profile.name);
            validate_llm_reasoning_effort(Some(value), &label)?;
        }

        let label = format!("profile '{}'", profile.name);
        validate_llm_provider_api_pair(profile_provider_kind, profile_api_kind, &label)?;
    }

    if cfg.defaults.stt_provider == "local" {
        // For local whisper, `stt_model` must be a path to a whisper.cpp GGML `.bin` model.
        let p = std::path::Path::new(&cfg.defaults.stt_model);
        if !p.exists() {
            return Err(format!(
                "local STT model does not exist: {}",
                cfg.defaults.stt_model
            ));
        }

        // If the file is GGUF, return a clearer error (this is a common migration issue).
        if voicewin_runtime::models::has_gguf_magic(p).unwrap_or(false) {
            return Err(format!(
                "local STT model is GGUF (.gguf), but VoiceWin local STT requires whisper.cpp GGML (.bin) models: {}",
                cfg.defaults.stt_model
            ));
        }

        voicewin_runtime::models::validate_ggml_file(p, 1024 * 1024).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
async fn get_config(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AppConfig, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let mut cfg = load_or_init_config(svc, &app)?;
    // Reflect current secret-store state (not just what's stored on disk).
    cfg.llm_api_key_present = llm_api_key_present(svc);
    Ok(cfg)
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

    #[cfg(any(windows, target_os = "macos"))]
    let (previous_microphone_id, previous_microphone_name) = svc
        .load_config()
        .ok()
        .map(|existing| {
            (
                existing.defaults.microphone_device_id,
                existing.defaults.microphone_device,
            )
        })
        .unwrap_or((None, None));

    // Normalize known model filenames in our app models dir.
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    if let Some(normalized) =
        normalize_model_path_to_models_dir(&app_data_dir, &cfg.defaults.stt_model)
    {
        cfg.defaults.stt_model = normalized;
    }

    // Never trust the frontend for secret state; refresh the key-present bit from storage.
    cfg.llm_api_key_present = llm_api_key_present(svc);
    voicewin_runtime::defaults::backfill_default_prompts(&mut cfg);

    validate_config(&cfg)?;

    svc.save_config(&cfg).map_err(|e| e.to_string())?;

    #[cfg(any(windows, target_os = "macos"))]
    {
        if voicewin_appcore::service::microphone_selection_changed(
            previous_microphone_id.as_deref(),
            previous_microphone_name.as_deref(),
            cfg.defaults.microphone_device_id.as_deref(),
            cfg.defaults.microphone_device.as_deref(),
        ) {
            svc.invalidate_recorder().await;
        }
    }

    Ok(())
}

#[tauri::command]
async fn preview_prompt(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    req: voicewin_runtime::ipc::PromptPreviewRequest,
) -> Result<voicewin_runtime::ipc::PromptPreviewResponse, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    if let Some(context_override) = req.context_override {
        let app_id = match svc.get_foreground_app().await {
            Ok(app_id) => app_id,
            Err(e) => {
                log::warn!("preview_prompt could not capture foreground app; using defaults: {e}");
                voicewin_core::types::AppIdentity::new()
            }
        };
        let snapshot = apply_prompt_context_override(
            svc.capture_context_snapshot().await,
            context_override,
        );
        svc.preview_prompt_with_app_snapshot(
            req.prompt,
            req.transcript,
            app_id,
            snapshot,
            req.forced_profile_id,
            req.force_defaults,
        )
        .await
        .map_err(|e| e.to_string())
    } else {
        svc.preview_prompt(
            req.prompt,
            req.transcript,
            req.forced_profile_id,
            req.force_defaults,
        )
        .await
        .map_err(|e| e.to_string())
    }
}

fn apply_prompt_context_override(
    mut snapshot: voicewin_engine::traits::ContextSnapshot,
    override_ctx: voicewin_runtime::ipc::PromptPreviewContextOverride,
) -> voicewin_engine::traits::ContextSnapshot {
    if let Some(value) = override_ctx
        .clipboard
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        snapshot.clipboard = Some(value.to_string());
    }
    if let Some(value) = override_ctx
        .selected_text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        snapshot.selected_text = Some(value.to_string());
    }
    if let Some(value) = override_ctx
        .window_context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        snapshot.window_context = Some(value.to_string());
    }
    if let Some(value) = override_ctx
        .screenshot_data_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        snapshot.screenshot = Some(voicewin_core::context::ImageArtifact {
            data_url: value.to_string(),
        });
    }
    snapshot
}

#[tauri::command]
async fn probe_llm_provider(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    req: voicewin_runtime::ipc::ProviderProbeRequest,
) -> Result<voicewin_runtime::ipc::ProviderProbeResponse, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.probe_llm_provider(
        &req.provider_kind,
        &req.api_kind,
        &req.base_url,
        &req.model,
        req.reasoning_effort.as_deref(),
        req.probe_kind,
    )
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ForegroundAppInfo {
    process_name: Option<String>,
    exe_path: Option<String>,
    window_title: Option<String>,
}

#[tauri::command]
async fn capture_foreground_app(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ForegroundAppInfo, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let app_id = svc.get_foreground_app().await.map_err(|e| e.to_string())?;

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
    log::info!("cancel_recording invoked");
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
    log::info!("toggle_recording invoked");
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    Ok(state.session.toggle_recording(&app, svc.clone()).await)
}

#[tauri::command]
async fn get_session_status(
    state: State<'_, AppState>,
) -> Result<session_controller::SessionStatusPayload, String> {
    Ok(state.session.get_status().await)
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
async fn delete_history_entry(
    app: tauri::AppHandle,
    ts_unix_ms: i64,
    text: String,
) -> Result<bool, String> {
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    store
        .delete_entry(ts_unix_ms, &text)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_history_entry_by_id(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    store.delete_entry_by_id(&id).map_err(|e| e.to_string())
}

fn history_replay_transcript(entry: &voicewin_runtime::history::HistoryEntry) -> Option<String> {
    [
        entry.raw_transcript.as_deref(),
        Some(entry.text.as_str()),
        entry.enhanced_text.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .map(ToOwned::to_owned)
}

fn history_replay_prompt(
    cfg: &AppConfig,
    entry: &voicewin_runtime::history::HistoryEntry,
) -> Option<voicewin_core::enhancement::PromptTemplate> {
    let by_id = entry.prompt_id.as_ref().and_then(|prompt_id| {
        cfg.prompts
            .iter()
            .find(|prompt| prompt.id.0.to_string() == *prompt_id)
            .cloned()
    });
    if by_id.is_some() {
        return by_id;
    }

    let by_title = entry.prompt_title.as_ref().and_then(|prompt_title| {
        cfg.prompts
            .iter()
            .find(|prompt| prompt.title == *prompt_title)
            .cloned()
    });
    if by_title.is_some() {
        return by_title;
    }

    cfg.defaults
        .prompt_id
        .as_ref()
        .and_then(|prompt_id| cfg.prompts.iter().find(|prompt| prompt.id == *prompt_id))
        .cloned()
        .or_else(|| cfg.prompts.first().cloned())
}

fn history_replay_app(
    entry: &voicewin_runtime::history::HistoryEntry,
) -> voicewin_core::types::AppIdentity {
    let mut app = voicewin_core::types::AppIdentity::new();
    if let Some(exe_path) = entry
        .app_exe_path
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        app = app.with_exe_path(exe_path.to_string());
    }
    if let Some(process_name) = entry
        .app_process_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        app = app.with_process_name(process_name.to_string());
    }
    if let Some(window_title) = entry
        .app_window_title
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        app = app.with_window_title(window_title.to_string());
    }
    app
}

fn history_replay_snapshot(
    entry: &voicewin_runtime::history::HistoryEntry,
) -> voicewin_engine::traits::ContextSnapshot {
    let process_name = entry
        .app_process_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let window_title = entry
        .app_window_title
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    let window_context = if process_name.is_none() && window_title.is_none() {
        None
    } else {
        Some(format!(
            "Application: {}\nActive Window: {}",
            process_name.unwrap_or("unknown"),
            window_title.unwrap_or("")
        ))
    };

    voicewin_engine::traits::ContextSnapshot {
        window_context,
        ..Default::default()
    }
}

#[tauri::command]
async fn preview_history_entry(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    id: String,
) -> Result<voicewin_runtime::ipc::PromptPreviewResponse, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let cfg = load_or_init_config(svc, &app)?;
    let path = default_history_path(&app).map_err(|e| e.to_string())?;
    let store = voicewin_runtime::history::HistoryStore::at_path(path);
    let entries = store.load().map_err(|e| e.to_string())?;
    let entry = entries
        .into_iter()
        .rfind(|entry| entry.id == id)
        .ok_or_else(|| format!("history entry not found: {id}"))?;

    let prompt = history_replay_prompt(&cfg, &entry)
        .ok_or_else(|| "no prompt available for history replay".to_string())?;
    let transcript = history_replay_transcript(&entry)
        .ok_or_else(|| "history entry has no replayable transcript".to_string())?;

    svc.preview_prompt_with_app_snapshot(
        prompt,
        transcript,
        history_replay_app(&entry),
        history_replay_snapshot(&entry),
        None,
        false,
    )
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct ModelStatus {
    pub bootstrap_ok: bool,
    pub bootstrap_path: String,
    pub preferred_ok: bool,
    pub preferred_path: String,
}

#[derive(serde::Serialize)]
struct ProviderStatus {
    pub openai_api_key_present: bool,
    pub openai_api_key_error: Option<String>,
    pub gemini_api_key_present: bool,
    pub gemini_api_key_error: Option<String>,
    pub elevenlabs_api_key_present: bool,
    pub elevenlabs_api_key_error: Option<String>,
}

fn provider_status(svc: &AppService) -> ProviderStatus {
    let (openai_api_key_present, openai_api_key_error) = match svc.get_openai_api_key_present() {
        Ok(v) => (v, None),
        Err(e) => (false, Some(e.to_string())),
    };

    let (gemini_api_key_present, gemini_api_key_error) = match svc.get_gemini_api_key_present() {
        Ok(v) => (v, None),
        Err(e) => (false, Some(e.to_string())),
    };

    let (elevenlabs_api_key_present, elevenlabs_api_key_error) =
        match svc.get_elevenlabs_api_key_present() {
            Ok(v) => (v, None),
            Err(e) => (false, Some(e.to_string())),
        };

    ProviderStatus {
        openai_api_key_present,
        openai_api_key_error,
        gemini_api_key_present,
        gemini_api_key_error,
        elevenlabs_api_key_present,
        elevenlabs_api_key_error,
    }
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
        ..provider_status(&svc)
    })
}

#[tauri::command]
async fn set_openai_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("API key cannot be empty. Use Clear to remove it.".into());
    }

    svc.set_openai_api_key(trimmed).map_err(|e| e.to_string())?;

    let status = provider_status(&svc);
    if let Some(e) = &status.openai_api_key_error {
        return Err(format!(
            "Saved key but failed to verify secret storage state: {e}"
        ));
    }
    if !status.openai_api_key_present {
        return Err("Saved key but it is still not present in secret storage.".into());
    }

    Ok(status)
}

#[tauri::command]
async fn clear_openai_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.clear_openai_api_key().map_err(|e| e.to_string())?;
    Ok(provider_status(&svc))
}

#[tauri::command]
async fn set_gemini_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("API key cannot be empty. Use Clear to remove it.".into());
    }

    svc.set_gemini_api_key(trimmed).map_err(|e| e.to_string())?;

    let status = provider_status(&svc);
    if let Some(e) = &status.gemini_api_key_error {
        return Err(format!(
            "Saved key but failed to verify secret storage state: {e}"
        ));
    }
    if !status.gemini_api_key_present {
        return Err("Saved key but it is still not present in secret storage.".into());
    }

    Ok(status)
}

#[tauri::command]
async fn clear_gemini_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.clear_gemini_api_key().map_err(|e| e.to_string())?;
    Ok(provider_status(&svc))
}

#[tauri::command]
async fn set_elevenlabs_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("API key cannot be empty. Use Clear to remove it.".into());
    }

    svc.set_elevenlabs_api_key(trimmed)
        .map_err(|e| e.to_string())?;

    let status = provider_status(&svc);
    if let Some(e) = &status.elevenlabs_api_key_error {
        return Err(format!(
            "Saved key but failed to verify secret storage state: {e}"
        ));
    }
    if !status.elevenlabs_api_key_present {
        return Err("Saved key but it is still not present in secret storage.".into());
    }

    Ok(status)
}

#[tauri::command]
async fn clear_elevenlabs_api_key(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ProviderStatus, String> {
    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    svc.clear_elevenlabs_api_key().map_err(|e| e.to_string())?;
    Ok(provider_status(&svc))
}

#[cfg(any(windows, target_os = "macos"))]
#[tauri::command]
async fn list_microphones() -> Result<Vec<String>, String> {
    AudioRecorder::list_input_device_names().map_err(|e| e.to_string())
}

#[cfg(any(windows, target_os = "macos"))]
#[derive(serde::Serialize)]
struct MicrophoneDevice {
    id: String,
    name: String,
    is_default: bool,
    is_selected: bool,
}

#[cfg(any(windows, target_os = "macos"))]
#[tauri::command]
async fn list_microphone_devices(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<Vec<MicrophoneDevice>, String> {
    fn normalize(value: Option<String>) -> Option<String> {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    let svc = state
        .service
        .get_or_try_init(|| async { build_service(&app).await })
        .await
        .map_err(|e| e.to_string())?;

    let cfg = svc.load_config().ok();
    let selected_id = normalize(
        cfg.as_ref()
            .and_then(|c| c.defaults.microphone_device_id.clone()),
    );
    let selected_name = normalize(
        cfg.as_ref()
            .and_then(|c| c.defaults.microphone_device.clone()),
    );

    let devices = AudioRecorder::list_input_devices().map_err(|e| e.to_string())?;
    let mut selected_found = false;

    let mut mapped = devices
        .into_iter()
        .map(
            |AudioInputDeviceInfo {
                 id,
                 name,
                 is_default,
             }| {
                let by_id = selected_id.as_ref().is_some_and(|needle| needle == &id);
                let by_name = selected_id.is_none()
                    && selected_name.as_ref().is_some_and(|needle| needle == &name);
                let is_selected = by_id || by_name;
                if is_selected {
                    selected_found = true;
                }

                MicrophoneDevice {
                    id,
                    name,
                    is_default,
                    is_selected,
                }
            },
        )
        .collect::<Vec<_>>();

    if !selected_found && selected_id.is_none() && selected_name.is_none() {
        if let Some(default_device) = mapped.iter_mut().find(|d| d.is_default) {
            default_device.is_selected = true;
        }
    }

    Ok(mapped)
}

#[tauri::command]
async fn get_model_status(app: tauri::AppHandle) -> Result<ModelStatus, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    let bootstrap_path = voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir);
    let preferred_path =
        voicewin_runtime::models::installed_preferred_local_stt_model_path(&app_data_dir);

    let bootstrap_ok = voicewin_runtime::models::validate_bootstrap_model(&bootstrap_path).is_ok();
    let preferred_ok =
        voicewin_runtime::models::validate_ggml_file(&preferred_path, 50 * 1024 * 1024).is_ok();

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

    fn paths_equivalent(a: &std::path::Path, b: &std::path::Path) -> bool {
        if a == b {
            return true;
        }

        #[cfg(windows)]
        {
            fn norm(p: &std::path::Path) -> String {
                let mut s = p.to_string_lossy().to_string();
                // Normalize separators + casing.
                s = s.replace('/', "\\");
                let lower = s.to_ascii_lowercase();
                // Strip Windows verbatim prefix if present.
                lower.strip_prefix("\\\\?\\").unwrap_or(&lower).to_string()
            }

            return norm(a) == norm(b);
        }

        #[cfg(not(windows))]
        {
            false
        }
    }

    let mut out = Vec::new();

    // Include the bundled bootstrap model as a selectable entry.
    let bootstrap_path = voicewin_runtime::models::installed_bootstrap_model_path(&app_data_dir);
    let bootstrap_size = std::fs::metadata(&bootstrap_path).map(|m| m.len()).ok();
    let bootstrap_installed =
        voicewin_runtime::models::validate_ggml_file(&bootstrap_path, 1024 * 1024).is_ok();
    let bootstrap_active = paths_equivalent(&active_path, &bootstrap_path);
    out.push(ModelCatalogEntry {
        id: BUNDLED_TINY_MODEL_ID.into(),
        title: "Whisper Tiny (Bundled)".into(),
        recommended: false,
        filename: voicewin_runtime::models::BOOTSTRAP_MODEL_FILENAME.into(),
        size_bytes: bootstrap_size,
        speed_label: Some("Fast".into()),
        accuracy_label: Some("OK".into()),
        installed: bootstrap_installed,
        active: bootstrap_installed && bootstrap_active,
        downloading: false,
    });

    for spec in voicewin_runtime::models::whisper_catalog() {
        let path = models_dir.join(&spec.filename);
        let installed = path.exists();
        let active = installed && paths_equivalent(&active_path, &path);

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

    if model_id == BUNDLED_TINY_MODEL_ID {
        // Ensure the bundled model exists; if the user deleted it, restore from app resources.
        let path = ensure_bootstrap_model(&app).map_err(|e| e.to_string())?;
        cfg.defaults.stt_provider = "local".into();
        cfg.defaults.stt_model = path.to_string_lossy().to_string();
        validate_config(&cfg)?;
        return svc.save_config(&cfg).map_err(|e| e.to_string());
    }

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
    log::info!("download_model start: {model_id}");
    let downloading =
        DOWNLOADING_MODELS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));

    {
        let mut guard = downloading
            .lock()
            .map_err(|_| "download lock poisoned".to_string())?;
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
        log::info!("download_model dst: {}", dst.display());
        log::info!("download_model url: {}", spec.url);
        if let Some(alt) = &spec.alt_url {
            log::info!("download_model url (fallback): {}", alt);
        }
        let expected_sha = spec.sha256.to_lowercase();

        // Stream download into a temp file.
        let tmp = dst.with_extension("download");
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }

        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;

        let client = reqwest::Client::new();

        let mut last_err: Option<String> = None;
        let mut used_url = spec.url.clone();
        let resp = match client.get(&spec.url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_err = Some(format!("download failed: status={}", r.status().as_u16()));
                if let Some(alt) = &spec.alt_url {
                    used_url = alt.clone();
                    client.get(alt).send().await.map_err(|e| e.to_string())?
                } else {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(last_err.unwrap_or_else(|| "download failed".into()));
                }
            }
            Err(e) => {
                last_err = Some(e.to_string());
                if let Some(alt) = &spec.alt_url {
                    used_url = alt.clone();
                    client.get(alt).send().await.map_err(|e| e.to_string())?
                } else {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(last_err.unwrap_or_else(|| "download failed".into()));
                }
            }
        };

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let _ = std::fs::remove_file(&tmp);
            if let Some(prev) = last_err {
                return Err(format!("download failed: {prev}; fallback status={status}"));
            }
            return Err(format!("download failed: status={status}"));
        }

        log::info!("download_model using url: {}", used_url);

        let total = resp.content_length();
        log::info!("download_model content_length: {:?}", total);
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

        // Basic sanity (GGML magic + non-trivial size).
        if let Err(e) = voicewin_runtime::models::validate_ggml_file(&tmp, 10 * 1024 * 1024) {
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

    match &result {
        Ok(()) => log::info!("download_model done: {model_id}"),
        Err(e) => log::error!("download_model failed: {model_id}: {e}"),
    }

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
                    let x =
                        work.position.x + (work.size.width as i32 / 2) - (size.width as i32 / 2);

                    // Place the pill so its bottom is 80px above the monitor bottom.
                    // (We align the window bottom accordingly; the webview itself includes shadow padding.)
                    let y = work.position.y + work.size.height as i32
                        - OVERLAY_BOTTOM_OFFSET
                        - (size.height as i32);

                    let _ = w.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn overlay_ready(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    // The overlay webview calls this after it has mounted and registered event listeners.
    // This lets us re-emit the current session status and avoid "missed first emit" races.
    state.session.mark_overlay_ready(&app).await;
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
    let status = Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("failed to open Accessibility settings".into())
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct MacosPermissionsStatus {
    accessibility_trusted: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
struct PlatformCapabilities {
    platform: &'static str,
    foreground_app_identity: bool,
    clipboard_context: bool,
    selected_text_context: bool,
    window_context: bool,
    screenshot_capture: bool,
    foreground_window_capture: bool,
    auto_insert: bool,
}

#[tauri::command]
async fn get_platform_capabilities() -> Result<PlatformCapabilities, String> {
    #[cfg(windows)]
    {
        Ok(PlatformCapabilities {
            platform: "windows",
            foreground_app_identity: true,
            clipboard_context: true,
            selected_text_context: true,
            window_context: true,
            screenshot_capture: true,
            foreground_window_capture: true,
            auto_insert: true,
        })
    }

    #[cfg(target_os = "macos")]
    {
        Ok(PlatformCapabilities {
            platform: "macos",
            foreground_app_identity: true,
            clipboard_context: true,
            selected_text_context: true,
            window_context: true,
            screenshot_capture: true,
            foreground_window_capture: false,
            auto_insert: true,
        })
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Ok(PlatformCapabilities {
            platform: "linux",
            foreground_app_identity: false,
            clipboard_context: true,
            selected_text_context: false,
            window_context: false,
            screenshot_capture: false,
            foreground_window_capture: false,
            auto_insert: false,
        })
    }
}

#[tauri::command]
async fn get_macos_permissions_status() -> Result<MacosPermissionsStatus, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(MacosPermissionsStatus {
            accessibility_trusted: voicewin_platform::macos::accessibility_trusted(),
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("macOS only".into())
    }
}

#[tauri::command]
async fn prompt_macos_accessibility_permission() -> Result<MacosPermissionsStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let trusted = voicewin_platform::macos::prompt_accessibility_permission();
        Ok(MacosPermissionsStatus {
            accessibility_trusted: trusted,
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("macOS only".into())
    }
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn open_macos_microphone_settings() -> Result<(), String> {
    use std::process::Command;

    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
    let status = Command::new("/usr/bin/open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("failed to open Microphone settings".into())
    }
}

fn emit_runtime_smoke_process_output(marker: &str) {
    let mut stdout = std::io::stdout();
    if let Err(error) = runtime_smoke::write_runtime_smoke_process_output(&mut stdout, marker) {
        log::error!("failed to write runtime smoke output: {error}");
    } else {
        let _ = std::io::Write::flush(&mut stdout);
    }
    log::info!("{marker}");
}

fn emit_runtime_smoke_start_process_output(smoke_mode: &runtime_smoke::RuntimeSmokeMode) {
    let mut stdout = std::io::stdout();
    if let Err(error) = runtime_smoke::write_runtime_smoke_start_process_output(
        &mut stdout,
        smoke_mode,
        env!("CARGO_PKG_VERSION"),
        BUILD_GIT_SHA,
    ) {
        log::error!("failed to write runtime smoke start output: {error}");
    } else {
        let _ = std::io::Write::flush(&mut stdout);
    }
    log::info!("{}", smoke_mode.start_marker);
}

fn request_smoke_process_exit(handle: tauri::AppHandle, exit_code: i32) {
    handle.exit(exit_code);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        std::process::exit(exit_code);
    });
}

async fn capture_runtime_smoke_foreground_until_match(
    svc: &AppService,
    expected_process_name: Option<&str>,
) -> Result<(Option<String>, bool), anyhow::Error> {
    let deadline =
        std::time::Instant::now() + runtime_smoke::RUNTIME_SMOKE_FOREGROUND_MATCH_TIMEOUT;
    let first_foreground = svc.get_foreground_app().await?;
    let mut last_actual_process_name = first_foreground
        .process_name
        .as_ref()
        .map(|name| name.0.clone());

    if runtime_smoke::foreground_process_matches(
        last_actual_process_name.as_deref(),
        expected_process_name,
    ) {
        return Ok((last_actual_process_name, true));
    }

    loop {
        let foreground = svc.get_foreground_app().await?;
        let actual_process_name = foreground.process_name.as_ref().map(|name| name.0.clone());

        if runtime_smoke::foreground_process_matches(
            actual_process_name.as_deref(),
            expected_process_name,
        ) {
            return Ok((actual_process_name, true));
        }

        last_actual_process_name = actual_process_name;
        if std::time::Instant::now() >= deadline {
            return Ok((last_actual_process_name, false));
        }

        tokio::time::sleep(runtime_smoke::RUNTIME_SMOKE_FOREGROUND_POLL_INTERVAL).await;
    }
}

async fn run_runtime_smoke(
    app: tauri::AppHandle,
    smoke_mode: Result<runtime_smoke::RuntimeSmokeMode, String>,
) -> i32 {
    let smoke_mode = match smoke_mode {
        Ok(smoke_mode) => smoke_mode,
        Err(error) => {
            log::error!("runtime smoke env invalid: {error}");
            emit_runtime_smoke_process_output(&runtime_smoke::runtime_smoke_failure_marker(
                env!("CARGO_PKG_VERSION"),
                BUILD_GIT_SHA,
                "invalid_env",
            ));
            return 2;
        }
    };

    emit_runtime_smoke_start_process_output(&smoke_mode);
    emit_runtime_smoke_process_output(
        &smoke_mode.stage_marker(runtime_smoke::RUNTIME_SMOKE_STAGE_REFOCUS_DELAY),
    );
    tokio::time::sleep(runtime_smoke::RUNTIME_SMOKE_REFOCUS_DELAY).await;

    emit_runtime_smoke_process_output(
        &smoke_mode.stage_marker(runtime_smoke::RUNTIME_SMOKE_STAGE_BUILD_SERVICE),
    );
    let (svc, smoke_cfg) = match build_runtime_smoke_service(&app).await {
        Ok(value) => value,
        Err(error) => {
            log::error!("runtime smoke build_service failed: {error}");
            emit_runtime_smoke_process_output(&smoke_mode.failure_marker("build_service"));
            return 1;
        }
    };

    if smoke_mode.expected_foreground_process.is_some() {
        emit_runtime_smoke_process_output(
            &smoke_mode.stage_marker(runtime_smoke::RUNTIME_SMOKE_STAGE_CAPTURE_FOREGROUND),
        );

        let (actual_process_name, matched) = match capture_runtime_smoke_foreground_until_match(
            &svc,
            smoke_mode.expected_foreground_process.as_deref(),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                log::error!("runtime smoke foreground capture failed: {error}");
                emit_runtime_smoke_process_output(&smoke_mode.failure_marker("foreground_capture"));
                return 1;
            }
        };

        if !matched {
            log::error!(
                "runtime smoke foreground mismatch after retry window: expected={:?} actual={:?}",
                smoke_mode.expected_foreground_process.as_deref(),
                actual_process_name.as_deref(),
            );
            emit_runtime_smoke_process_output(&smoke_mode.failure_marker("foreground_mismatch"));
            return 1;
        }
    }

    emit_runtime_smoke_process_output(
        &smoke_mode.stage_marker(runtime_smoke::RUNTIME_SMOKE_STAGE_SESSION),
    );
    let response = match svc
        .run_session_with_hook_using_config(
            smoke_cfg,
            voicewin_runtime::ipc::RunSessionRequest {
                transcript: smoke_mode.transcript.clone(),
                warning: None,
            },
            voicewin_engine::traits::AudioInput {
                sample_rate_hz: 16_000,
                samples: Vec::new(),
            },
            |stage| {
                let marker = runtime_smoke::runtime_smoke_stage_marker(stage);
                async move {
                    emit_runtime_smoke_process_output(&marker);
                }
            },
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            log::error!("runtime smoke session execution failed: {error}");
            emit_runtime_smoke_process_output(&smoke_mode.failure_marker("run_session"));
            return 1;
        }
    };

    emit_runtime_smoke_process_output(&smoke_mode.stage_marker(&response.stage));
    if response.stage == "done" {
        if let Some(warning) = response
            .warning
            .as_deref()
            .filter(|warning| !warning.trim().is_empty())
            .or_else(|| {
                response
                    .error
                    .as_deref()
                    .filter(|warning| !warning.trim().is_empty())
            })
        {
            log::warn!("runtime smoke completed with non-fatal warning: {warning}");
        }
        emit_runtime_smoke_process_output(&smoke_mode.success_marker);
        return 0;
    }

    if let Some(error) = response.error.as_deref() {
        log::error!(
            "runtime smoke failed: stage={} error={error}",
            response.stage
        );
        if let Some(warning) = response.warning.as_deref().filter(|warning| !warning.trim().is_empty())
        {
            log::warn!("runtime smoke failure carried warning: {warning}");
        }
    } else if let Some(warning) = response
        .warning
        .as_deref()
        .filter(|warning| !warning.trim().is_empty())
    {
        log::error!(
            "runtime smoke failed: stage={} warning={warning}",
            response.stage
        );
    } else {
        log::error!("runtime smoke failed: stage={}", response.stage);
    }
    emit_runtime_smoke_process_output(&smoke_mode.failure_marker("run_session"));
    1
}

fn main() {
    let startup_smoke_enabled = std::env::var(startup_smoke::STARTUP_SMOKE_ENABLE_ENV).ok();
    let smoke_mode = startup_smoke::startup_smoke_mode(
        startup_smoke_enabled.as_deref(),
        env!("CARGO_PKG_VERSION"),
        BUILD_GIT_SHA,
    );
    let runtime_smoke_enabled = std::env::var(runtime_smoke::RUNTIME_SMOKE_ENABLE_ENV).ok();
    let runtime_smoke_transcript = std::env::var(runtime_smoke::RUNTIME_SMOKE_TRANSCRIPT_ENV).ok();
    let runtime_smoke_expected_process =
        std::env::var(runtime_smoke::RUNTIME_SMOKE_EXPECT_PROCESS_ENV).ok();
    let runtime_smoke_requested =
        runtime_smoke::runtime_smoke_enabled(runtime_smoke_enabled.as_deref());
    let runtime_smoke_mode = runtime_smoke::runtime_smoke_mode(
        runtime_smoke_enabled.as_deref(),
        runtime_smoke_transcript.as_deref(),
        runtime_smoke_expected_process.as_deref(),
        env!("CARGO_PKG_VERSION"),
        BUILD_GIT_SHA,
    );
    let skip_single_instance = smoke_mode.is_some() || runtime_smoke_requested;

    // If we crash/panic on end-user machines, a stderr backtrace is often not available.
    // Write panics to a predictable temp file to aid debugging.
    {
        use std::io::Write;
        use std::sync::OnceLock;

        static PANIC_LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
        let _ = PANIC_LOG_PATH.set(std::env::temp_dir().join("voicewin_panic.log"));

        std::panic::set_hook(Box::new(|info| {
            let Some(path) = PANIC_LOG_PATH.get() else {
                return;
            };
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{info}");
            }
        }));
    }

    // Persist logs to the OS log dir so Windows users can debug issues even in
    // `windows_subsystem = "windows"` builds (no console output).
    use tauri_plugin_log::{Target, TargetKind};

    let mut builder = tauri::Builder::default()
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
        .plugin(tauri_plugin_global_shortcut::Builder::new().build());
    if !skip_single_instance {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // If a second instance is launched, bring the existing window to the front.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }

    builder
        .manage(AppState {
            service: Arc::new(tokio::sync::OnceCell::new()),
            session: SessionController::new(),

            #[cfg(any(windows, target_os = "macos"))]
            toggle_hotkey: std::sync::Mutex::new(DEFAULT_TOGGLE_HOTKEY.into()),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            preview_prompt,
            probe_llm_provider,
            toggle_recording,
            cancel_recording,
            get_session_status,
            #[cfg(any(windows, target_os = "macos"))]
            get_toggle_hotkey,
            #[cfg(any(windows, target_os = "macos"))]
            set_toggle_hotkey,
            get_history,
            clear_history,
            delete_history_entry,
            delete_history_entry_by_id,
            preview_history_entry,
            get_provider_status,
            set_openai_api_key,
            clear_openai_api_key,
            set_gemini_api_key,
            clear_gemini_api_key,
            set_elevenlabs_api_key,
            clear_elevenlabs_api_key,
            get_model_status,
            #[cfg(any(windows, target_os = "macos"))]
            list_microphones,
            #[cfg(any(windows, target_os = "macos"))]
            list_microphone_devices,
            list_models,
            download_model,
            set_active_model,
            capture_foreground_app,
            overlay_drag_begin,
            overlay_drag_end,
            overlay_set_size,
            overlay_ready,
            overlay_dismiss,
            show_main_window,
            get_platform_capabilities,
            get_macos_permissions_status,
            prompt_macos_accessibility_permission,
            #[cfg(target_os = "macos")]
            open_macos_accessibility_settings,
            #[cfg(target_os = "macos")]
            open_macos_microphone_settings,
        ])
        .setup(move |app| {
            log::info!(
                "{}",
                startup_smoke::startup_provenance_log(env!("CARGO_PKG_VERSION"), BUILD_GIT_SHA)
            );

            if let Some(smoke_mode) = smoke_mode.as_ref() {
                let mut stdout = std::io::stdout();
                if let Err(error) = startup_smoke::write_startup_smoke_process_output(
                    &mut stdout,
                    smoke_mode,
                    env!("CARGO_PKG_VERSION"),
                    BUILD_GIT_SHA,
                ) {
                    log::error!("failed to write startup smoke output: {error}");
                }
                let _ = std::io::Write::flush(&mut stdout);
                log::info!("{}", smoke_mode.marker);
                request_smoke_process_exit(app.handle().clone(), 0);
                return Ok(());
            }

            if runtime_smoke_requested {
                if let Some(main_window) = app.get_webview_window("main") {
                    let _ = main_window.hide();
                }

                let handle = app.handle().clone();
                let runtime_smoke_mode = runtime_smoke_mode.clone().and_then(|mode| {
                    mode.ok_or_else(|| "runtime smoke requested but mode was unavailable".to_string())
                });
                tauri::async_runtime::spawn(async move {
                    let exit_code = run_runtime_smoke(handle.clone(), runtime_smoke_mode).await;
                    request_smoke_process_exit(handle, exit_code);
                });
                return Ok(());
            }

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

            // Do NOT apply Acrylic to the overlay window.
            // It affects the entire webview surface, making the overlay look like a grey rectangle
            // instead of a floating pill. We rely on the CSS pill styling instead.

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

                    let WindowEvent::Moved(pos) = event else {
                        return;
                    };

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
            let open_logs = MenuItemBuilder::new("Open Logs Folder")
                .id("open_logs")
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
                    &open_logs,
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
                    // Clone the MenuItem handle before the `move` closure so we can
                    // keep using `toggle` later (e.g. for the global hotkey handler).
                    let toggle_for_tray_menu = toggle.clone();
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
                            let toggle_item = toggle_for_tray_menu.clone();

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
                        "open_logs" => {
                            // Best-effort: open the app log directory in the OS file manager.
                            // Do not fail silently; show a dialog on error.
                            #[cfg(target_os = "macos")]
                            {
                                use std::process::Command;

                                let mut opened = false;
                                let mut errors: Vec<String> = Vec::new();

                                let log_dir = match app.path().app_log_dir() {
                                    Ok(dir) => Some(dir),
                                    Err(e) => {
                                        errors.push(format!("failed to resolve log dir: {e}"));
                                        None
                                    }
                                };

                                if let Some(dir) = log_dir.as_ref() {
                                    let log_file = dir.join("voicewin.log");

                                    let res = if log_file.exists() {
                                        Command::new("/usr/bin/open")
                                            .arg("-R")
                                            .arg(&log_file)
                                            .status()
                                            .map(|s| (s, format!("reveal {}", log_file.display())))
                                    } else {
                                        Command::new("/usr/bin/open")
                                            .arg(dir)
                                            .status()
                                            .map(|s| (s, format!("open {}", dir.display())))
                                    };

                                    match res {
                                        Ok((status, _)) if status.success() => {
                                            opened = true;
                                        }
                                        Ok((status, label)) => {
                                            errors.push(format!("/usr/bin/open ({label}) exited with {status}"));
                                        }
                                        Err(e) => {
                                            errors.push(format!("/usr/bin/open failed: {e}"));
                                        }
                                    }
                                }

                                // Fall back to the panic log file in temp.
                                if !opened {
                                    let p = std::env::temp_dir().join("voicewin_panic.log");
                                    if p.exists() {
                                        match Command::new("/usr/bin/open")
                                            .arg("-R")
                                            .arg(&p)
                                            .status()
                                        {
                                            Ok(status) if status.success() => {
                                                opened = true;
                                            }
                                            Ok(status) => {
                                                errors.push(format!(
                                                    "/usr/bin/open (reveal {}) exited with {status}",
                                                    p.display()
                                                ));
                                            }
                                            Err(e) => {
                                                errors.push(format!(
                                                    "/usr/bin/open (reveal {}) failed: {e}",
                                                    p.display()
                                                ));
                                            }
                                        }
                                    }
                                }

                                if !opened {
                                    let log_dir_display = log_dir
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|| "<unavailable>".into());

                                    let panic_log = std::env::temp_dir().join("voicewin_panic.log");

                                    let msg = format!(
                                        "Could not open logs.\n\nLog dir: {log_dir_display}\nExpected log file: {log_dir_display}/voicewin.log\nPanic log: {}\n\nErrors:\n{}",
                                        panic_log.display(),
                                        errors.join("\n")
                                    );

                                    log::error!("open_logs failed: {msg}");

                                    app.dialog()
                                        .message(msg)
                                        .title("VoiceWin")
                                        .kind(MessageDialogKind::Error)
                                        .buttons(MessageDialogButtons::Ok)
                                        .show(|_| {});
                                }
                            }

                            #[cfg(windows)]
                            {
                                if let Ok(dir) = app.path().app_log_dir() {
                                    let _ =
                                        std::process::Command::new("explorer").arg(dir).status();
                                }

                                // If that fails, fall back to the panic log file in temp.
                                let p = std::env::temp_dir().join("voicewin_panic.log");
                                if p.exists() {
                                    let _ = std::process::Command::new("explorer").arg(p).status();
                                }
                            }

                            #[cfg(all(not(windows), not(target_os = "macos")))]
                            {
                                if let Ok(dir) = app.path().app_log_dir() {
                                    let _ =
                                        std::process::Command::new("xdg-open").arg(dir).status();
                                }
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
                                        let x = work.position.x + (work.size.width as i32 / 2)
                                            - (size.width as i32 / 2);

                                        // Align the overlay window bottom so the pill appears ~80px above the
                                        // monitor bottom (the window itself includes shadow padding).
                                        let y = work.position.y + work.size.height as i32
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
                let toggle_for_hotkey = toggle.clone();

                match app_handle.global_shortcut().on_shortcut(
                    hotkey.as_str(),
                    move |app, _shortcut, event| {
                        if event.state != ShortcutState::Pressed {
                            return;
                        }

                        let app = app.clone();
                        let session = session.clone();
                        let svc_cell = svc_cell.clone();
                        let toggle_for_hotkey = toggle_for_hotkey.clone();

                        tauri::async_runtime::spawn(async move {
                            let svc = match svc_cell
                                .get_or_try_init(|| async { build_service(&app).await })
                                .await
                            {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!("hotkey service init failed: {e}");
                                    return;
                                }
                            };

                            let res = session.toggle_recording(&app, svc.clone()).await;
                            let _ = toggle_for_hotkey.set_text(if res.is_recording {
                                "Stop Recording"
                            } else {
                                "Start Recording"
                            });
                        });
                    },
                ) {
                    Ok(_) => log::info!("registered hotkey: {hotkey}"),
                    Err(e) => log::error!("failed to register hotkey {hotkey}: {e}"),
                }
            }

            let _ = tray;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

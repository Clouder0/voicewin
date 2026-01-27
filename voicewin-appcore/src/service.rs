use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use voicewin_core::config::AppConfig;
use voicewin_engine::engine::VoicewinEngine;
use voicewin_engine::traits::{AppContextProvider, AudioInput, Inserter};

#[cfg(any(windows, target_os = "macos"))]
use voicewin_audio::{AudioCaptureError, AudioRecorder};
use voicewin_runtime::config_store::ConfigStore;
use voicewin_runtime::ipc::{RunSessionRequest, RunSessionResponse};

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
use voicewin_runtime::secrets::{SecretKey, delete_secret, get_secret, set_secret};

#[derive(Clone)]
pub struct AppService {
    config_store: ConfigStore,
    ctx: Arc<dyn AppContextProvider>,
    inserter: Arc<dyn Inserter>,

    #[cfg(any(windows, target_os = "macos"))]
    recorder: Arc<tokio::sync::Mutex<Option<AudioRecorder>>>,
}

impl AppService {
    pub fn new(
        config_path: PathBuf,
        ctx: Arc<dyn AppContextProvider>,
        inserter: Arc<dyn Inserter>,
    ) -> Self {
        Self {
            config_store: ConfigStore::at_path(config_path),
            ctx,
            inserter,
            #[cfg(any(windows, target_os = "macos"))]
            recorder: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub async fn start_recording(&self) -> Result<(), AudioCaptureError> {
        let mut recorder = self.recorder.lock().await;
        if recorder.is_none() {
            let cfg = self.load_config().ok();
            let preferred = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device.as_deref());
            *recorder = Some(AudioRecorder::open_named(preferred)?);
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
            let preferred = cfg
                .as_ref()
                .and_then(|c| c.defaults.microphone_device.as_deref());
            *recorder = Some(AudioRecorder::open_named(preferred)?);
        }
        let r = recorder.as_ref().ok_or(AudioCaptureError::NoInputDevice)?;

        r.set_level_callback(cb);
        r.start()
    }

    pub fn load_config(&self) -> anyhow::Result<AppConfig> {
        self.config_store.load()
    }

    pub fn save_config(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        self.config_store.save(cfg)
    }

    pub fn set_openai_api_key(&self, value: &str) -> anyhow::Result<()> {
        set_secret(SecretKey::OpenAiCompatibleApiKey, value)
    }

    pub fn get_openai_api_key_present(&self) -> anyhow::Result<bool> {
        Ok(get_secret(SecretKey::OpenAiCompatibleApiKey)?.is_some())
    }

    pub fn set_elevenlabs_api_key(&self, value: &str) -> anyhow::Result<()> {
        set_secret(SecretKey::ElevenLabsApiKey, value)
    }

    pub fn get_elevenlabs_api_key_present(&self) -> anyhow::Result<bool> {
        Ok(get_secret(SecretKey::ElevenLabsApiKey)?.is_some())
    }

    pub fn clear_api_keys(&self) -> anyhow::Result<()> {
        delete_secret(SecretKey::OpenAiCompatibleApiKey)?;
        delete_secret(SecretKey::ElevenLabsApiKey)?;
        Ok(())
    }

    pub async fn get_foreground_app(&self) -> anyhow::Result<voicewin_core::types::AppIdentity> {
        self.ctx.foreground_app().await
    }

    pub async fn run_session(
        &self,
        req: RunSessionRequest,
        audio: AudioInput,
    ) -> anyhow::Result<RunSessionResponse> {
        self.run_session_with_hook(req, audio, |_stage| async {}).await
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

        // Design-draft UI treats History as always enabled.
        // Keep the config flag for backward compatibility, but it must not disable history.
        let history_enabled = true;
        let _ = cfg.defaults.history_enabled;

        let engine: VoicewinEngine =
            build_engine_from_config(cfg, self.ctx.clone(), self.inserter.clone()).await?;

        // Reserved for future use (e.g. transcript override/debug).
        let _ = req;

        // Run the full session pipeline and emit stage progress.
        let res = engine.run_session_with_hook(audio, on_stage).await;

        let (stage, final_text, error) = match res {
            Ok(result) => {
                let stage = result
                    .stage_label
                    .unwrap_or_else(|| format!("{:?}", result.stage).to_lowercase());
                (stage, result.final_text, result.error)
            }
            Err(e) => {
                // On any failure, rely on History for recovery.
                ("error".into(), None, Some(e.to_string()))
            }
        };

        if history_enabled {
            let should_write = final_text
                .as_ref()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false)
                || error.is_some();

            if should_write {
                let text = final_text.clone().unwrap_or_default();
                let ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;

                let app = self.ctx.foreground_app().await.ok();

                let entry = voicewin_runtime::history::HistoryEntry {
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
                    stage: stage.clone(),
                    error: error.clone(),
                };

                // Best-effort: write history alongside config.
                let history_path = self
                    .config_store
                    .path()
                    .parent()
                    .map(|p| p.join("history.json"))
                    .unwrap_or_else(|| PathBuf::from("history.json"));

                let store = voicewin_runtime::history::HistoryStore::at_path(history_path);
                let _ = store.append(entry);
            }
        }

        Ok(RunSessionResponse {
            stage,
            final_text,
            error,
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_core::enhancement::{PromptMode, PromptTemplate};
    use voicewin_core::power_mode::GlobalDefaults;
    use voicewin_core::types::{InsertMode, PromptId};

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
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                microphone_device: None,
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
                },
                audio,
            )
            .await;
    }
}

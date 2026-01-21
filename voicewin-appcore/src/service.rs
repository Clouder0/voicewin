use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use voicewin_core::config::AppConfig;
use voicewin_engine::engine::VoicewinEngine;
use voicewin_engine::traits::{AppContextProvider, AudioInput, Inserter};

#[cfg(windows)]
use voicewin_audio::{AudioCaptureError, AudioRecorder};
use voicewin_runtime::config_store::ConfigStore;
use voicewin_runtime::ipc::{RunSessionRequest, RunSessionResponse};

#[cfg(windows)]
use voicewin_runtime::ipc::{RecordingStatus, ToggleRecordingResponse};
use voicewin_runtime::runtime_engine::build_engine_from_config;
use voicewin_runtime::secrets::{delete_secret, get_secret, set_secret, SecretKey};

#[derive(Clone)]
pub struct AppService {
    config_store: ConfigStore,
    ctx: Arc<dyn AppContextProvider>,
    inserter: Arc<dyn Inserter>,

    #[cfg(windows)]
    recorder: Arc<tokio::sync::Mutex<Option<AudioRecorder>>>,

}

impl AppService {
    pub fn new(config_path: PathBuf, ctx: Arc<dyn AppContextProvider>, inserter: Arc<dyn Inserter>) -> Self {
        Self {
            config_store: ConfigStore::at_path(config_path),
            ctx,
            inserter,
            #[cfg(windows)]
            recorder: Arc::new(tokio::sync::Mutex::new(AudioRecorder::open_default().ok())),

        }
    }

    #[cfg(windows)]
    pub async fn start_recording(&self) -> Result<(), AudioCaptureError> {
        let mut recorder = self.recorder.lock().await;
        let r = recorder
            .as_mut()
            .ok_or(AudioCaptureError::NoInputDevice)?;
        r.start()
    }

    #[cfg(windows)]
    pub async fn stop_recording(&self) -> Result<AudioInput, AudioCaptureError> {
        let mut recorder = self.recorder.lock().await;
        let r = recorder
            .as_mut()
            .ok_or(AudioCaptureError::NoInputDevice)?;

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

    #[cfg(windows)]
    pub async fn cancel_recording(&self) -> Result<(), AudioCaptureError> {
        // Best-effort: stop and discard captured audio.
        let mut recorder = self.recorder.lock().await;
        let r = recorder
            .as_mut()
            .ok_or(AudioCaptureError::NoInputDevice)?;

        let _ = r.stop();
        Ok(())
    }

    #[cfg(windows)]
    pub async fn start_recording_with_level_callback<F>(&self, cb: F) -> Result<(), AudioCaptureError>
    where
        F: Fn(&[f32]) + Send + Sync + 'static,
    {
        // Set callback first, then start.
        let mut recorder = self.recorder.lock().await;
        let r = recorder
            .as_mut()
            .ok_or(AudioCaptureError::NoInputDevice)?;

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

    pub async fn run_session(&self, req: RunSessionRequest, audio: AudioInput) -> anyhow::Result<RunSessionResponse> {
        let cfg = self.config_store.load()?;
        let history_enabled = cfg.defaults.history_enabled;

        let engine: VoicewinEngine = build_engine_from_config(cfg, self.ctx.clone(), self.inserter.clone()).await?;

        // Reserved for future use (e.g. transcript override/debug).
        let _ = req;

        let result = engine.run_session(audio).await?;

        let stage = result
            .stage_label
            .unwrap_or_else(|| format!("{:?}", result.stage).to_lowercase());

        if history_enabled {
            if let Some(text) = result.final_text.clone() {
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
            final_text: result.final_text,
            error: result.error,
        })
    }

    #[cfg(windows)]
    pub async fn toggle_recording(&self) -> ToggleRecordingResponse {
        // Simple toggle: if we were recording, stop and run.
        // Otherwise start recording.
        static IS_RECORDING: std::sync::OnceLock<std::sync::atomic::AtomicBool> =
            std::sync::OnceLock::new();
        let flag = IS_RECORDING.get_or_init(|| std::sync::atomic::AtomicBool::new(false));

        let was_recording = flag.swap(true, std::sync::atomic::Ordering::SeqCst);
        if !was_recording {
            // Start recording.
            if let Err(e) = self.start_recording().await {
                flag.store(false, std::sync::atomic::Ordering::SeqCst);
                return ToggleRecordingResponse {
                    status: RecordingStatus { is_recording: false },
                    stage: "error".into(),
                    final_text: None,
                    error: Some(e.to_string()),
                };
            }

            return ToggleRecordingResponse {
                status: RecordingStatus { is_recording: true },
                stage: "recording".into(),
                final_text: None,
                error: None,
            };
        }

        // Stop recording.
        flag.store(false, std::sync::atomic::Ordering::SeqCst);
        let audio = match self.stop_recording().await {
            Ok(a) => a,
            Err(e) => {
                return ToggleRecordingResponse {
                    status: RecordingStatus { is_recording: false },
                    stage: "error".into(),
                    final_text: None,
                    error: Some(e.to_string()),
                };
            }
        };

        let res = self
            .run_session(RunSessionRequest { transcript: String::new() }, audio)
            .await;

        match res {
            Ok(r) => ToggleRecordingResponse {
                status: RecordingStatus { is_recording: false },
                stage: r.stage,
                final_text: r.final_text,
                error: r.error,
            },
            Err(e) => ToggleRecordingResponse {
                status: RecordingStatus { is_recording: false },
                stage: "error".into(),
                final_text: None,
                error: Some(e.to_string()),
            },
        }
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

        // This will fail because local model path is missing, but validates plumbing.
        let res = svc
            .run_session(RunSessionRequest { transcript: "hi".into() }, audio)
            .await;
        assert!(res.is_err());
    }
}

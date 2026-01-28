use std::sync::Arc;

use voicewin_core::config::AppConfig;
use voicewin_engine::engine::{EngineConfig, VoicewinEngine};
use voicewin_engine::traits::{AppContextProvider, Inserter, LlmProvider, SttProvider};

use crate::llm::OpenAiCompatibleLlmProvider;
use crate::local_stt::LocalWhisperSttProvider;
use crate::secrets::{SecretKey, get_secret};
use crate::stt::ElevenLabsSttProvider;
use crate::stt_router::SttRouter;

/// Build a runnable engine from config + platform providers.
///
/// This keeps the Tauri layer thin.
pub async fn build_engine_from_config(
    cfg: AppConfig,
    ctx: Arc<dyn AppContextProvider>,
    inserter: Arc<dyn Inserter>,
) -> anyhow::Result<VoicewinEngine> {
    // Secrets (OS keyring)
    let llm_api_key = get_secret(SecretKey::OpenAiCompatibleApiKey)?.unwrap_or_default();
    let eleven_key = get_secret(SecretKey::ElevenLabsApiKey)?.unwrap_or_default();

    let llm: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleLlmProvider::new(llm_api_key.clone()));

    // STT router
    let local: Arc<dyn SttProvider> = Arc::new(LocalWhisperSttProvider::new());
    let eleven: Arc<dyn SttProvider> = Arc::new(ElevenLabsSttProvider::new(eleven_key));

    // Wrap router as a provider.
    let router = Arc::new(RouterProvider {
        router: SttRouter::new().with_local(local).with_elevenlabs(eleven),
    });

    let engine_cfg = EngineConfig {
        defaults: cfg.defaults,
        profiles: cfg.profiles,
        prompts: cfg.prompts,
        // Keep the key in the engine config so the pipeline can decide whether
        // enhancement is possible. The actual provider still owns the secret at runtime.
        llm_api_key,
    };

    Ok(VoicewinEngine::new(engine_cfg, ctx, router, llm, inserter))
}

#[derive(Clone)]
struct RouterProvider {
    router: SttRouter,
}

#[async_trait::async_trait]
impl SttProvider for RouterProvider {
    async fn transcribe(
        &self,
        audio: &voicewin_engine::traits::AudioInput,
        provider: &str,
        model: &str,
        language: &str,
    ) -> anyhow::Result<voicewin_engine::traits::Transcript> {
        self.router
            .transcribe(audio, provider, model, language)
            .await
    }
}

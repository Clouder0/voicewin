use std::sync::Arc;

use voicewin_engine::traits::{AudioInput, SttProvider, Transcript};

/// Simple STT router that dispatches based on the `provider` string.
///
/// MVP supported providers:
/// - "local" -> local Whisper (filesystem model path)
/// - "elevenlabs" -> ElevenLabs cloud STT
#[derive(Clone)]
pub struct SttRouter {
    local: Option<Arc<dyn SttProvider>>,
    elevenlabs: Option<Arc<dyn SttProvider>>,
}

impl SttRouter {
    pub fn new() -> Self {
        Self {
            local: None,
            elevenlabs: None,
        }
    }

    pub fn with_local(mut self, provider: Arc<dyn SttProvider>) -> Self {
        self.local = Some(provider);
        self
    }

    pub fn with_elevenlabs(mut self, provider: Arc<dyn SttProvider>) -> Self {
        self.elevenlabs = Some(provider);
        self
    }

    pub async fn transcribe(
        &self,
        audio: &AudioInput,
        provider: &str,
        model: &str,
        language: &str,
    ) -> anyhow::Result<Transcript> {
        match provider {
            "local" => self
                .local
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("local STT provider not configured"))?
                .transcribe(audio, provider, model, language)
                .await,
            "elevenlabs" => self
                .elevenlabs
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("ElevenLabs STT provider not configured"))?
                .transcribe(audio, provider, model, language)
                .await,
            other => Err(anyhow::anyhow!("unsupported STT provider: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeStt;

    #[async_trait::async_trait]
    impl SttProvider for FakeStt {
        async fn transcribe(
            &self,
            _audio: &AudioInput,
            provider: &str,
            model: &str,
            language: &str,
        ) -> anyhow::Result<Transcript> {
            Ok(Transcript {
                text: format!("p={provider} m={model} l={language}"),
                provider: provider.into(),
                model: model.into(),
            })
        }
    }

    #[tokio::test]
    async fn routes_to_configured_provider() {
        let router = SttRouter::new().with_elevenlabs(Arc::new(FakeStt));
        let audio = AudioInput {
            sample_rate_hz: 16_000,
            samples: vec![0.0; 4],
        };

        let t = router
            .transcribe(&audio, "elevenlabs", "scribe_v1", "en")
            .await
            .unwrap();
        assert!(t.text.contains("scribe_v1"));
    }
}

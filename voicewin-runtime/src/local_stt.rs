use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use voicewin_engine::traits::{AudioInput, Transcript};

#[derive(Clone)]
pub struct LocalWhisperSttProvider {
    cache: Arc<Mutex<Option<CachedModel>>>,
}

struct CachedModel {
    model_path: PathBuf,
    ctx: Arc<WhisperContext>,
}

impl Default for LocalWhisperSttProvider {
    fn default() -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
        }
    }
}

impl LocalWhisperSttProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn get_or_load_context(&self, model_path: &PathBuf) -> anyhow::Result<Arc<WhisperContext>> {
        let mut guard = self.cache.lock().unwrap();

        if let Some(cached) = guard.as_ref() {
            if cached.model_path == *model_path {
                return Ok(cached.ctx.clone());
            }
        }

        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "local whisper model does not exist: {}",
                model_path.display()
            ));
        }

        // User-friendly error: whisper-rs (whisper.cpp) expects the legacy GGML `.bin` format.
        // Our app previously used GGUF models; detect that early so the error is actionable.
        if crate::models::has_gguf_magic(model_path.as_path()).unwrap_or(false) {
            return Err(anyhow::anyhow!(
                "local whisper model is GGUF (.gguf), but the local engine requires whisper.cpp GGML (.bin) models: {}",
                model_path.display()
            ));
        }

        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid model path"))?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| anyhow::anyhow!("failed to load whisper model: {e}"))?;

        let ctx = Arc::new(ctx);
        *guard = Some(CachedModel {
            model_path: model_path.clone(),
            ctx: ctx.clone(),
        });
        Ok(ctx)
    }

    fn transcribe_blocking(
        &self,
        audio: &AudioInput,
        model_path: PathBuf,
        language: &str,
    ) -> anyhow::Result<String> {
        if audio.sample_rate_hz != 16_000 {
            return Err(anyhow::anyhow!(
                "unsupported sample rate {} (expected 16000)",
                audio.sample_rate_hz
            ));
        }

        let ctx = self.get_or_load_context(&model_path)?;
        let mut state = ctx
            .create_state()
            .map_err(|e| anyhow::anyhow!("failed to create whisper state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        if language != "auto" {
            params.set_language(Some(language));
        }

        // Keep console output disabled.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, &audio.samples)
            .map_err(|e| anyhow::anyhow!("whisper inference failed: {e}"))?;

        let n = state.full_n_segments();

        let mut out = String::new();
        for i in 0..n {
            let seg = state
                .get_segment(i)
                .ok_or_else(|| anyhow::anyhow!("failed reading whisper segment {i}: out of bounds"))?;
            let text = seg
                .to_str_lossy()
                .map_err(|e| anyhow::anyhow!("failed reading whisper segment {i}: {e}"))?;
            out.push_str(text.trim());
            if i + 1 < n {
                out.push(' ');
            }
        }

        Ok(out.trim().to_string())
    }
}

#[async_trait::async_trait]
impl voicewin_engine::traits::SttProvider for LocalWhisperSttProvider {
    async fn transcribe(
        &self,
        audio: &AudioInput,
        provider: &str,
        model: &str,
        language: &str,
    ) -> anyhow::Result<Transcript> {
        if provider != "local" {
            return Err(anyhow::anyhow!("unsupported STT provider: {provider}"));
        }

        // MVP convention: for local whisper, `model` is a filesystem path to a whisper.cpp GGML `.bin` model.
        let model_path = PathBuf::from(model);

        let text = tokio::task::spawn_blocking({
            let this = self.clone();
            let audio = audio.clone();
            let language = language.to_string();
            move || this.transcribe_blocking(&audio, model_path, &language)
        })
        .await
        .map_err(|e| anyhow::anyhow!("whisper task join failed: {e}"))??;

        Ok(Transcript {
            text,
            provider: provider.into(),
            model: model.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_engine::traits::SttProvider;

    #[tokio::test]
    async fn rejects_missing_model_path() {
        let stt = LocalWhisperSttProvider::new();
        let audio = AudioInput {
            sample_rate_hz: 16_000,
            samples: vec![0.0; 160],
        };

        let err = stt
            .transcribe(&audio, "local", "/definitely/does/not/exist.bin", "en")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn rejects_non_16khz_audio() {
        let stt = LocalWhisperSttProvider::new();
        let audio = AudioInput {
            sample_rate_hz: 48_000,
            samples: vec![0.0; 160],
        };

        let err = stt.transcribe(&audio, "local", "./model.bin", "en").await;
        assert!(err.is_err());
    }
}

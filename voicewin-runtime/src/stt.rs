use voicewin_engine::traits::{AudioInput, Transcript};

pub fn encode_wav_mono_f32le(samples: &[f32], sample_rate_hz: u32) -> Vec<u8> {
    // Simple WAV (RIFF) writer: 32-bit float PCM, mono.
    // Enough for cloud STT uploads.
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 32;
    let audio_format: u16 = 3; // IEEE float

    let byte_rate = sample_rate_hz * num_channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = num_channels * (bits_per_sample / 8);

    let data_bytes_len = samples.len() as u32 * 4;

    let mut out = Vec::with_capacity((44 + data_bytes_len) as usize);

    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&audio_format.to_le_bytes());
    out.extend_from_slice(&num_channels.to_le_bytes());
    out.extend_from_slice(&sample_rate_hz.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes_len.to_le_bytes());

    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }

    out
}

#[derive(Clone)]
pub struct ElevenLabsSttProvider {
    api_key: String,
}

impl std::fmt::Debug for ElevenLabsSttProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevenLabsSttProvider")
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl ElevenLabsSttProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

#[async_trait::async_trait]
impl voicewin_engine::traits::SttProvider for ElevenLabsSttProvider {
    async fn transcribe(
        &self,
        audio: &AudioInput,
        provider: &str,
        model: &str,
        language: &str,
    ) -> anyhow::Result<Transcript> {
        if provider != "elevenlabs" {
            return Err(anyhow::anyhow!("unsupported STT provider: {provider}"));
        }

        let cfg = voicewin_providers::elevenlabs::ElevenLabsSttConfig {
            api_key: self.api_key.clone(),
            model_id: model.to_string(),
            language_code: match language {
                "auto" => None,
                other => Some(other.to_string()),
            },
        };

        if cfg.api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("missing ElevenLabs API key"));
        }

        let wav = encode_wav_mono_f32le(&audio.samples, audio.sample_rate_hz);

        let req = voicewin_providers::elevenlabs::build_elevenlabs_stt_request(
            &cfg,
            &voicewin_providers::elevenlabs::AudioFile {
                filename: "input.wav".into(),
                mime_type: "audio/wav".into(),
                bytes: wav,
            },
        );

        let resp = voicewin_providers::runtime::execute(&req).await?;
        if !(200..=299).contains(&resp.status) {
            return Err(anyhow::anyhow!(
                "ElevenLabs STT failed: status={} body={}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ));
        }

        let text = voicewin_providers::parse::parse_elevenlabs_transcription(&resp.body)?;
        Ok(Transcript {
            text,
            provider: provider.into(),
            model: model.into(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MockSttProvider {
    pub text: String,
}

#[async_trait::async_trait]
impl voicewin_engine::traits::SttProvider for MockSttProvider {
    async fn transcribe(
        &self,
        _audio: &AudioInput,
        provider: &str,
        model: &str,
        _language: &str,
    ) -> anyhow::Result<Transcript> {
        Ok(Transcript {
            text: self.text.clone(),
            provider: provider.into(),
            model: model.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_has_basic_header() {
        let wav = encode_wav_mono_f32le(&[0.0, 1.0], 16_000);
        assert!(wav.starts_with(b"RIFF"));
        assert!(wav[8..12].eq(b"WAVE"));
        assert!(wav.windows(4).any(|w| w == b"fmt "));
        assert!(wav.windows(4).any(|w| w == b"data"));
    }
}

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, anyhow};
use voicewin_engine::traits::{AudioInput, SttProvider};
use voicewin_providers::elevenlabs_realtime::{
    ElevenLabsRealtimeConfig, RealtimeEvent, spawn_realtime_session,
};
use voicewin_runtime::stt::ElevenLabsSttProvider;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("testdata")
        .join("audio")
        .join("english.wav")
}

fn load_fixture_audio() -> anyhow::Result<AudioInput> {
    let path = fixture_path();
    let mut reader = hound::WavReader::open(&path)
        .with_context(|| format!("open fixture audio at {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(anyhow!(
            "expected mono fixture audio, got {} channels",
            spec.channels
        ));
    }

    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .context("read float WAV samples")?,
        hound::SampleFormat::Int => {
            let scale = ((1_i64 << (spec.bits_per_sample.saturating_sub(1) as u32)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|v| v as f32 / scale.max(1.0)))
                .collect::<Result<Vec<_>, _>>()
                .context("read PCM WAV samples")?
        }
    };

    Ok(AudioInput {
        sample_rate_hz: 16_000,
        samples: resample_linear_mono(&samples, spec.sample_rate, 16_000),
    })
}

fn resample_linear_mono(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input_rate == output_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let output_len = ((samples.len() as u64 * output_rate as u64) / input_rate as u64) as usize;
    let ratio = input_rate as f64 / output_rate as f64;
    let mut out = Vec::with_capacity(output_len.max(1));

    for idx in 0..output_len.max(1) {
        let src = idx as f64 * ratio;
        let base = src.floor() as usize;
        let next = (base + 1).min(samples.len() - 1);
        let frac = (src - base as f64) as f32;
        let sample = samples[base] * (1.0 - frac) + samples[next] * frac;
        out.push(sample);
    }

    out
}

fn assert_batch_transcript(transcript: &str) {
    let normalized = transcript.trim().to_lowercase();
    assert!(
        normalized.split_whitespace().count() >= 2,
        "expected at least two words from transcript, got: {}",
        transcript
    );
    assert!(
        normalized.chars().any(|ch| ch.is_ascii_alphabetic()),
        "expected alphabetic transcript content, got: {}",
        transcript
    );
}

fn assert_realtime_transcript(transcript: &str) {
    let normalized = transcript.trim().to_lowercase();
    assert!(
        !normalized.is_empty(),
        "expected non-empty realtime transcript, got: {}",
        transcript
    );
    assert!(
        normalized.chars().any(|ch| ch.is_ascii_alphanumeric()),
        "expected alphanumeric realtime transcript content, got: {}",
        transcript
    );
}

#[tokio::test]
#[ignore = "requires VOICEWIN_LIVE_PROVIDER_TESTS=1 and ELEVENLABS_SCRIBE_V2_API_KEY"]
async fn transcribes_fixture_with_live_elevenlabs_batch() {
    let api_key = std::env::var("ELEVENLABS_SCRIBE_V2_API_KEY")
        .expect("ELEVENLABS_SCRIBE_V2_API_KEY must be set for live smoke tests");
    let audio = load_fixture_audio().expect("fixture audio should load");

    let provider = ElevenLabsSttProvider::new(api_key);
    let transcript = provider
        .transcribe(&audio, "elevenlabs", "scribe_v2", "auto")
        .await
        .expect("live transcription should succeed");

    assert_batch_transcript(&transcript.text);
}

#[tokio::test]
#[ignore = "requires VOICEWIN_LIVE_PROVIDER_TESTS=1 and ELEVENLABS_SCRIBE_V2_API_KEY"]
async fn transcribes_fixture_with_live_elevenlabs_realtime() {
    let api_key = std::env::var("ELEVENLABS_SCRIBE_V2_API_KEY")
        .expect("ELEVENLABS_SCRIBE_V2_API_KEY must be set for live smoke tests");
    let audio = load_fixture_audio().expect("fixture audio should load");
    let cfg = ElevenLabsRealtimeConfig::production(api_key, audio.sample_rate_hz)
        .expect("production realtime config should build");

    let (handle, mut events) = spawn_realtime_session(cfg)
        .await
        .expect("live realtime session should connect");

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match events.recv().await {
                Some(RealtimeEvent::SessionStarted { .. }) => return Ok(()),
                Some(RealtimeEvent::Error {
                    message_type,
                    error,
                }) => {
                    return Err(anyhow!(
                        "realtime session failed before session_started: {message_type}: {error}"
                    ));
                }
                Some(_) => continue,
                None => return Err(anyhow!("realtime session closed before session_started")),
            }
        }
    })
    .await
    .expect("waiting for session_started should not time out")
    .expect("realtime session should start cleanly");

    let drain_task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if let RealtimeEvent::Error {
                message_type,
                error,
            } = event
            {
                return Err(anyhow!(
                    "realtime event stream reported error: {message_type}: {error}"
                ));
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    for chunk in audio.samples.chunks(800) {
        let sent = handle
            .send_audio_chunk(voicewin_runtime::stt::encode_pcm_s16le_mono(chunk))
            .await;
        assert!(sent, "realtime audio chunk should send");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let transcript = handle
        .finalize()
        .await
        .expect("live realtime finalize should succeed");
    handle.shutdown().await;
    drain_task
        .await
        .expect("realtime event drain task should join")
        .expect("realtime event stream should remain healthy");

    assert_realtime_transcript(&transcript);
}

#[test]
fn live_fixture_loader_resamples_to_16khz() {
    let audio = load_fixture_audio().expect("fixture audio should load");

    assert_eq!(audio.sample_rate_hz, 16_000);
    assert!(!audio.samples.is_empty());
}

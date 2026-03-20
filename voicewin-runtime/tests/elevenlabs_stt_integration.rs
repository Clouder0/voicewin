use std::sync::Mutex;

use voicewin_engine::traits::{AudioInput, SttProvider};
use voicewin_runtime::stt::ElevenLabsSttProvider;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct ScopedEnvVar {
    key: &'static str,
    original: Option<String>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: String) -> Self {
        let original = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, original }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match self.original.as_ref() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sixteen_khz_audio() -> AudioInput {
    AudioInput {
        sample_rate_hz: 16_000,
        // Keep the PCM payload UTF-8 safe so wiremock string body matchers can inspect it.
        samples: vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    }
}

#[tokio::test]
async fn transcribe_normalizes_realtime_model_and_hits_mock_server() {
    let _guard = env_guard();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/speech-to-text"))
        .and(header("xi-api-key", "test-key"))
        .and(body_string_contains("name=\"model_id\""))
        .and(body_string_contains("scribe_v2"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("{\"text\":\"fixture transcript\"}", "application/json"),
        )
        .mount(&server)
        .await;

    let _url = ScopedEnvVar::set(
        "VOICEWIN_ELEVENLABS_STT_URL",
        format!(" {}/speech-to-text ", server.uri()),
    );

    let provider = ElevenLabsSttProvider::new("test-key");
    let transcript = provider
        .transcribe(
            &sixteen_khz_audio(),
            "elevenlabs",
            "scribe_v2_realtime",
            "auto",
        )
        .await
        .expect("transcription should succeed");

    let requests = server
        .received_requests()
        .await
        .expect("request recording should be enabled");
    let body = String::from_utf8_lossy(&requests[0].body);

    assert_eq!(transcript.text, "fixture transcript");
    assert_eq!(transcript.provider, "elevenlabs");
    assert_eq!(transcript.model, "scribe_v2_realtime");
    assert!(body.contains("name=\"model_id\""));
    assert!(body.contains("scribe_v2"));
    assert!(body.contains("filename=\"input.pcm\""));
    assert!(body.contains("name=\"file_format\""));
    assert!(body.contains("pcm_s16le_16"));
    assert!(!body.contains("name=\"language_code\""));
}

#[tokio::test]
async fn transcribe_surfaces_non_success_status_with_response_body() {
    let _guard = env_guard();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/speech-to-text"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad api key"))
        .mount(&server)
        .await;

    let _url = ScopedEnvVar::set(
        "VOICEWIN_ELEVENLABS_STT_URL",
        format!("{}/speech-to-text", server.uri()),
    );

    let provider = ElevenLabsSttProvider::new("bad-key");
    let err = provider
        .transcribe(&sixteen_khz_audio(), "elevenlabs", "scribe_v2", "auto")
        .await
        .expect_err("non-success status should fail");

    let message = err.to_string();
    assert!(
        message.contains("status=401"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("bad api key"),
        "unexpected error: {message}"
    );
}

#[tokio::test]
async fn transcribe_rejects_non_16khz_audio_before_network() {
    let provider = ElevenLabsSttProvider::new("test-key");
    let err = provider
        .transcribe(
            &AudioInput {
                sample_rate_hz: 44_100,
                samples: vec![0.0, 0.1, -0.1],
            },
            "elevenlabs",
            "scribe_v2",
            "auto",
        )
        .await
        .expect_err("non-16kHz audio should fail before network");

    assert!(err.to_string().contains("16kHz audio"));
}

#[tokio::test]
async fn transcribe_rejects_blank_api_key_before_http() {
    let provider = ElevenLabsSttProvider::new("   ");
    let err = provider
        .transcribe(&sixteen_khz_audio(), "elevenlabs", "scribe_v2", "auto")
        .await
        .expect_err("blank api key should fail before http");

    assert!(err.to_string().contains("missing ElevenLabs API key"));
}

#[tokio::test]
async fn transcribe_includes_explicit_language_in_request_body() {
    let _guard = env_guard();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/speech-to-text"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("{\"text\":\"fixture transcript\"}", "application/json"),
        )
        .mount(&server)
        .await;

    let _url = ScopedEnvVar::set(
        "VOICEWIN_ELEVENLABS_STT_URL",
        format!("{}/speech-to-text", server.uri()),
    );

    let provider = ElevenLabsSttProvider::new("test-key");
    provider
        .transcribe(&sixteen_khz_audio(), "elevenlabs", "scribe_v2", "en")
        .await
        .expect("transcription should succeed");

    let requests = server
        .received_requests()
        .await
        .expect("request recording should be enabled");
    let body = String::from_utf8_lossy(&requests[0].body);

    assert!(body.contains("name=\"language_code\""));
    assert!(body.contains("en"));
}

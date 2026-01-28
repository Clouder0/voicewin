use crate::request::{Body, HttpRequest};

#[derive(Clone, PartialEq, Eq)]
pub struct ElevenLabsSttConfig {
    pub api_key: String,
    pub model_id: String,
    pub language_code: Option<String>,
}

impl std::fmt::Debug for ElevenLabsSttConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevenLabsSttConfig")
            .field("api_key", &"[REDACTED]")
            .field("model_id", &self.model_id)
            .field("language_code", &self.language_code)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFile {
    pub filename: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub fn build_elevenlabs_stt_request(cfg: &ElevenLabsSttConfig, audio: &AudioFile) -> HttpRequest {
    let boundary = format!("Boundary-{}", uuid::Uuid::new_v4());

    let mut body: Vec<u8> = Vec::new();

    append_file(
        &mut body,
        &boundary,
        "file",
        &audio.filename,
        &audio.mime_type,
        &audio.bytes,
    );
    append_field(&mut body, &boundary, "model_id", &cfg.model_id);
    append_field(&mut body, &boundary, "temperature", "0.0");
    append_field(&mut body, &boundary, "tag_audio_events", "false");

    if let Some(lang) = cfg.language_code.as_ref().filter(|s| !s.trim().is_empty()) {
        append_field(&mut body, &boundary, "language_code", lang);
    }

    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    HttpRequest {
        method: "POST".into(),
        url: "https://api.elevenlabs.io/v1/speech-to-text".into(),
        headers: vec![
            (
                "Content-Type".into(),
                format!("multipart/form-data; boundary={}", boundary),
            ),
            ("Accept".into(), "application/json".into()),
            ("xi-api-key".into(), cfg.api_key.clone()),
        ],
        body: Body::MultipartFormData {
            boundary,
            bytes: body,
        },
    }
}

fn append_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn append_file(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    mime_type: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
            name, filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", mime_type).as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_multipart_with_xi_api_key() {
        let cfg = ElevenLabsSttConfig {
            api_key: "k".into(),
            model_id: "scribe_v1".into(),
            language_code: Some("en".into()),
        };
        let audio = AudioFile {
            filename: "a.wav".into(),
            mime_type: "audio/wav".into(),
            bytes: vec![1, 2, 3],
        };
        let req = build_elevenlabs_stt_request(&cfg, &audio);
        assert_eq!(req.method, "POST");
        assert!(req.url.contains("elevenlabs"));
        assert_eq!(req.header("xi-api-key"), Some("k"));

        match req.body {
            Body::MultipartFormData { bytes, .. } => {
                let s = String::from_utf8_lossy(&bytes);
                assert!(s.contains("name=\"model_id\""));
                assert!(s.contains("scribe_v1"));
                assert!(s.contains("name=\"language_code\""));
                assert!(s.contains("en"));
            }
            _ => panic!("expected multipart"),
        }
    }
}

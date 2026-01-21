use anyhow::{Context, anyhow};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ElevenLabsTranscriptionResponse {
    pub text: String,
}

pub fn parse_elevenlabs_transcription(body: &[u8]) -> anyhow::Result<String> {
    let resp: ElevenLabsTranscriptionResponse =
        serde_json::from_slice(body).context("decode ElevenLabs JSON")?;
    Ok(resp.text)
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

pub fn parse_openai_chat_completion(body: &[u8]) -> anyhow::Result<String> {
    let resp: OpenAiChatResponse = serde_json::from_slice(body).context("decode chat JSON")?;
    let content = resp
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .ok_or_else(|| anyhow!("no content in chat completion response"))?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_elevenlabs_text() {
        let body = br#"{"text":"hello"}"#;
        assert_eq!(parse_elevenlabs_transcription(body).unwrap(), "hello");
    }

    #[test]
    fn parses_openai_chat_content() {
        let body = br#"{"choices":[{"message":{"content":"hi"}}]}"#;
        assert_eq!(parse_openai_chat_completion(body).unwrap(), "hi");
    }

    #[test]
    fn openai_missing_content_errors() {
        let body = br#"{"choices":[{"message":{}}]}"#;
        assert!(parse_openai_chat_completion(body).is_err());
    }
}

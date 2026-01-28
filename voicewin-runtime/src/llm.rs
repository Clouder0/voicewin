use voicewin_engine::traits::EnhancedText;

#[derive(Clone)]
pub struct OpenAiCompatibleLlmProvider {
    api_key: String,
}

impl std::fmt::Debug for OpenAiCompatibleLlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleLlmProvider")
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl OpenAiCompatibleLlmProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

#[async_trait::async_trait]
impl voicewin_engine::traits::LlmProvider for OpenAiCompatibleLlmProvider {
    async fn enhance(
        &self,
        base_url: &str,
        _api_key: &str,
        model: &str,
        system_message: &str,
        user_message: &str,
    ) -> anyhow::Result<EnhancedText> {
        let cfg = voicewin_providers::openai_compatible::OpenAiCompatibleChatConfig {
            base_url: base_url.to_string(),
            api_key: self.api_key.clone(),
            model: model.to_string(),
        };

        let messages = vec![
            voicewin_providers::openai_compatible::ChatMessage {
                role: "system".into(),
                content: system_message.to_string(),
            },
            voicewin_providers::openai_compatible::ChatMessage {
                role: "user".into(),
                content: user_message.to_string(),
            },
        ];

        let req =
            voicewin_providers::openai_compatible::build_chat_completions_request(&cfg, &messages);
        let resp = voicewin_providers::runtime::execute(&req).await?;

        if !(200..=299).contains(&resp.status) {
            return Err(anyhow::anyhow!(
                "OpenAI-compatible request failed: status={} body={}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ));
        }

        let text = voicewin_providers::parse::parse_openai_chat_completion(&resp.body)?;
        Ok(EnhancedText {
            text,
            provider: "openai-compatible".into(),
            model: model.into(),
        })
    }
}

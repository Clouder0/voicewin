use voicewin_core::context::ImageArtifact;
use voicewin_engine::traits::EnhancedText;

#[derive(Clone)]
pub struct VoicewinLlmProvider {
    http: voicewin_providers::runtime::HttpExecutor,
}

pub type OpenAiCompatibleLlmProvider = VoicewinLlmProvider;

impl std::fmt::Debug for VoicewinLlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoicewinLlmProvider").finish()
    }
}

impl VoicewinLlmProvider {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            http: voicewin_providers::runtime::HttpExecutor::new()?,
        })
    }

    pub async fn preflight_http_connect(
        &self,
        provider_kind: &str,
        base_url: &str,
        api_key: &str,
    ) -> anyhow::Result<()> {
        let req = match provider_kind.trim() {
            "gemini" => voicewin_providers::gemini::build_list_models_request(base_url, api_key),
            "openai_compatible" | "" => {
                voicewin_providers::openai_compatible::build_list_models_request(base_url, api_key)
            }
            other => {
                return Err(anyhow::anyhow!(
                    "unsupported llm provider kind for preflight: {other}"
                ));
            }
        };
        let resp = self.http.execute(&req).await?;

        if !(200..=299).contains(&resp.status) {
            return Err(anyhow::anyhow!(
                "LLM preflight /models failed: status={} body={}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl voicewin_engine::traits::LlmProvider for VoicewinLlmProvider {
    async fn enhance(
        &self,
        provider_kind: &str,
        api_kind: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        system_message: &str,
        user_message: &str,
        attached_image: Option<&ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        if let Some(image) = attached_image
            .as_ref()
            .filter(|image| !image.data_url.trim().is_empty())
        {
            voicewin_providers::data_url::parse_base64_image_data_url(&image.data_url)?;
        }

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

        match provider_kind.trim() {
            "gemini" => match api_kind.trim() {
                "stream_generate_content_sse" | "gemini_stream_sse" | "" => {
                    let cfg = voicewin_providers::gemini::GeminiGenerateContentConfig {
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        model: model.to_string(),
                        reasoning_effort: reasoning_effort.map(ToOwned::to_owned),
                    };

                    let req = voicewin_providers::gemini::build_stream_generate_content_request(
                        &cfg,
                        &messages,
                        attached_image,
                    );
                    let response = self
                        .http
                        .execute_sse_collect_text_with_metrics(
                            &req,
                            voicewin_providers::gemini::extract_generate_content_sse_text_event,
                        )
                        .await?;
                    Ok(EnhancedText {
                        text: response.text,
                        provider: "gemini".into(),
                        model: model.into(),
                        first_token_ms: response.first_text_ms,
                        input_tokens: response.input_tokens,
                        cached_input_tokens: response.cached_input_tokens,
                    })
                }
                other => Err(anyhow::anyhow!("unsupported Gemini api kind: {other}")),
            },
            "openai_compatible" | "" => match api_kind.trim() {
                "responses_sse" | "responses" => {
                    let cfg = voicewin_providers::openai_responses::OpenAiResponsesConfig {
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        model: model.to_string(),
                        reasoning_effort: reasoning_effort.map(ToOwned::to_owned),
                    };

                    let req = voicewin_providers::openai_responses::build_responses_sse_request(
                        &cfg,
                        &messages,
                        attached_image,
                    );
                    let response = self.http.execute_sse_collect_text_metrics(&req).await?;
                    Ok(EnhancedText {
                        text: response.text,
                        provider: "openai-responses".into(),
                        model: model.into(),
                        first_token_ms: response.first_text_ms,
                        input_tokens: response.input_tokens,
                        cached_input_tokens: response.cached_input_tokens,
                    })
                }
                "chat_completions" | "chat" | "" => {
                    if attached_image.is_some() {
                        anyhow::bail!(
                            "image input requires OpenAI Responses or Gemini streamGenerateContent"
                        );
                    }
                    let cfg = voicewin_providers::openai_compatible::OpenAiCompatibleChatConfig {
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        model: model.to_string(),
                        reasoning_effort: reasoning_effort.map(ToOwned::to_owned),
                    };

                    let req = voicewin_providers::openai_compatible::build_chat_completions_request(
                        &cfg, &messages,
                    );
                    let resp = self.http.execute(&req).await?;

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
                        first_token_ms: None,
                        input_tokens: None,
                        cached_input_tokens: None,
                    })
                }
                other => Err(anyhow::anyhow!("unsupported llm api kind: {other}")),
            },
            other => Err(anyhow::anyhow!("unsupported llm provider kind: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_engine::traits::LlmProvider;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn preflight_http_connect_uses_authenticated_models_get() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw("{\"data\":[]}", "application/json"),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatibleLlmProvider::new().expect("provider");
        provider
            .preflight_http_connect("openai_compatible", &server.uri(), "test-key")
            .await
            .expect("preflight should succeed");
    }

    #[tokio::test]
    async fn preflight_http_connect_returns_error_on_non_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(503).set_body_string("upstream unavailable"))
            .mount(&server)
            .await;

        let provider = OpenAiCompatibleLlmProvider::new().expect("provider");
        let err = provider
            .preflight_http_connect("openai_compatible", &server.uri(), "test-key")
            .await
            .expect_err("preflight should fail");

        assert!(err.to_string().contains("LLM preflight /models failed"));
        assert!(err.to_string().contains("503"));
    }

    #[tokio::test]
    async fn gemini_preflight_uses_x_goog_api_key_header() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("x-goog-api-key", "gem-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw("{\"models\":[]}", "application/json"),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatibleLlmProvider::new().expect("provider");
        provider
            .preflight_http_connect("gemini", &server.uri(), "gem-key")
            .await
            .expect("gemini preflight should succeed");
    }

    #[tokio::test]
    async fn enhance_dispatches_to_gemini_native_sse() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/models/gemini-3-flash-preview:streamGenerateContent"))
            .and(header("x-goog-api-key", "gem-key"))
            .and(header("accept", "text/event-stream"))
            .and(body_string_contains("Rewrite only."))
            .and(body_string_contains("hello voicewin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_string(
                        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"BANANA\"}]},\"finishReason\":\"STOP\"}]}\n\n",
                    ),
            )
            .mount(&server)
            .await;

        let provider = OpenAiCompatibleLlmProvider::new().expect("provider");
        let result = provider
            .enhance(
                "gemini",
                "stream_generate_content_sse",
                &server.uri(),
                "gem-key",
                "gemini-3-flash-preview",
                Some("low"),
                "Rewrite only.",
                "hello voicewin",
                None,
            )
            .await
            .expect("gemini enhance should succeed");

        assert_eq!(result.text, "BANANA");
        assert_eq!(result.provider, "gemini");
        assert_eq!(result.model, "gemini-3-flash-preview");
    }
}

use std::sync::Arc;
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::{GlobalDefaults, PowerModeOverrides, PowerModeProfile};
use voicewin_core::types::{AppIdentity, InsertMode, ProfileId, PromptId};
use voicewin_engine::engine::{EngineConfig, VoicewinEngine};
use voicewin_engine::traits::{
    AppContextProvider, AudioInput, ContextSnapshot, EnhancedText, Inserter, LlmProvider,
    SttProvider, Transcript,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestContext;

#[async_trait::async_trait]
impl AppContextProvider for TestContext {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(AppIdentity::new().with_process_name("slack.exe"))
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(ContextSnapshot {
            clipboard: Some("VOICE-123".into()),
            selected_text: None,
            window_context: Some("Application: Slack".into()),
            custom_vocabulary: Some("VoiceInk".into()),
        })
    }
}

struct TestInserter {
    inserted: Arc<std::sync::Mutex<Vec<(String, InsertMode)>>>,
}

#[async_trait::async_trait]
impl Inserter for TestInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        self.inserted.lock().unwrap().push((text.to_string(), mode));
        Ok(())
    }
}

struct TestStt;

#[async_trait::async_trait]
impl SttProvider for TestStt {
    async fn transcribe(
        &self,
        _audio: &AudioInput,
        provider: &str,
        model: &str,
        _language: &str,
    ) -> anyhow::Result<Transcript> {
        Ok(Transcript {
            text: "rewrite um hello world rewrite".into(),
            provider: provider.into(),
            model: model.into(),
        })
    }
}

struct OpenAiCompatibleLlm;

#[async_trait::async_trait]
impl LlmProvider for OpenAiCompatibleLlm {
    async fn enhance(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        system_message: &str,
        user_message: &str,
    ) -> anyhow::Result<EnhancedText> {
        let cfg = voicewin_providers::openai_compatible::OpenAiCompatibleChatConfig {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
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
            return Err(anyhow::anyhow!("bad status {}", resp.status));
        }

        let text = voicewin_providers::parse::parse_openai_chat_completion(&resp.body)?;
        Ok(EnhancedText {
            text,
            provider: "openai-compatible".into(),
            model: model.into(),
        })
    }
}

#[tokio::test]
async fn end_to_end_session_uses_power_mode_and_llm() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"choices":[{"message":{"content":"Hello, world."}}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let defaults = GlobalDefaults {
        enable_enhancement: true,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "mock".into(),
        language: "en".into(),
        llm_base_url: server.uri(),
        llm_model: "gpt-4o-mini".into(),
        microphone_device: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    };

    let profile = PowerModeProfile {
        id: ProfileId::new(),
        name: "Slack".into(),
        enabled: true,
        matchers: vec![voicewin_core::power_mode::AppMatcher::ProcessNameEquals(
            "slack.exe".into(),
        )],
        overrides: PowerModeOverrides {
            insert_mode: Some(InsertMode::PasteAndEnter),
            ..Default::default()
        },
    };

    let prompts = vec![PromptTemplate {
        id: PromptId::new(),
        title: "Rewrite".into(),
        mode: PromptMode::Enhancer,
        prompt_text: "Clean up.".into(),
        trigger_words: vec!["rewrite".into()],
    }];

    let inserted = Arc::new(std::sync::Mutex::new(vec![]));

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles: vec![profile],
            prompts,
            llm_api_key: "k".into(),
        },
        Arc::new(TestContext),
        Arc::new(TestStt),
        Arc::new(OpenAiCompatibleLlm),
        Arc::new(TestInserter {
            inserted: inserted.clone(),
        }),
    );

    let audio = AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 8],
    };

    let res = engine.run_session(audio).await.unwrap();
    assert_eq!(res.final_text.as_deref(), Some("Hello, world."));

    let inserted = inserted.lock().unwrap();
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].0, "Hello, world.");
    assert_eq!(inserted[0].1, InsertMode::PasteAndEnter);
}

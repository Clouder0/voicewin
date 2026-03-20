use std::sync::Arc;
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::{GlobalDefaults, PowerModeOverrides, PowerModeProfile};
use voicewin_core::types::{AppIdentity, InsertMode, ProfileId, PromptId};
use voicewin_engine::engine::{EngineConfig, VoicewinEngine};
use voicewin_engine::traits::{
    AppContextProvider, AudioInput, ContextSnapshot, EnhancedText, Inserter, LlmProvider,
    SttProvider, Transcript,
};
use voicewin_providers::openai_compatible::{
    ChatMessage, OpenAiCompatibleChatConfig, build_chat_completions_request,
};
use voicewin_providers::parse::parse_openai_chat_completion;
use voicewin_providers::runtime;

struct DummyContextProvider;

#[async_trait::async_trait]
impl AppContextProvider for DummyContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(AppIdentity::new()
            .with_process_name("slack.exe")
            .with_window_title("Daily standup"))
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        Ok(ContextSnapshot {
            clipboard: Some("Ticket: VOICE-123".into()),
            selected_text: None,
            window_context: Some("Application: Slack\nActive Window: Daily standup".into()),
            custom_vocabulary: Some("VoiceInk, ElevenLabs, Power Mode".into()),
            screenshot: None,
            screenshot_metadata: None,
            precomputed_screen_ocr: None,
        })
    }
}

struct DummyInserter;

#[async_trait::async_trait]
impl Inserter for DummyInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        println!("[insert:{:?}] {}", mode, text);
        Ok(())
    }
}

struct MockSttProvider;

#[async_trait::async_trait]
impl SttProvider for MockSttProvider {
    async fn transcribe(
        &self,
        _audio: &AudioInput,
        provider: &str,
        model: &str,
        _language: &str,
    ) -> anyhow::Result<Transcript> {
        Ok(Transcript {
            text: "rewrite um hello team this is a quick update rewrite".into(),
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
        _provider_kind: &str,
        _api_kind: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        _reasoning_effort: Option<&str>,
        system_message: &str,
        user_message: &str,
        _attached_image: Option<&voicewin_core::context::ImageArtifact>,
    ) -> anyhow::Result<EnhancedText> {
        // Build request using our provider module and call it.
        let cfg = OpenAiCompatibleChatConfig {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            reasoning_effort: _reasoning_effort.map(ToOwned::to_owned),
        };

        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system_message.to_string(),
            },
            ChatMessage {
                role: "user".into(),
                content: user_message.to_string(),
            },
        ];

        let req = build_chat_completions_request(&cfg, &messages);
        let resp = runtime::execute(&req).await?;
        if !(200..=299).contains(&resp.status) {
            return Err(anyhow::anyhow!(
                "LLM request failed: status={} body={}",
                resp.status,
                String::from_utf8_lossy(&resp.body)
            ));
        }

        let text = parse_openai_chat_completion(&resp.body)?;
        Ok(EnhancedText {
            text,
            provider: "openai-compatible".into(),
            model: model.into(),
            first_token_ms: None,
            input_tokens: None,
            cached_input_tokens: None,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MVP CLI behavior: run an end-to-end session using mock STT + real LLM call.
    // If you don't want network calls, set LLM_API_KEY="" and we will skip enhancement.

    let llm_api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    let llm_base_url =
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "http://localhost:11434/v1".into());
    let llm_model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-5.4".into());
    let llm_api_kind = std::env::var("LLM_API_KIND").unwrap_or_else(|_| "responses_sse".into());
    let llm_reasoning_effort = std::env::var("LLM_REASONING_EFFORT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let defaults = GlobalDefaults {
        enable_enhancement: !llm_api_key.trim().is_empty(),
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "mock".into(),
        language: "en".into(),
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url,
        llm_model,
        llm_api_kind,
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort,
        microphone_device: None,
        microphone_device_id: None,
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
        prompt_text: "Clean up grammar and punctuation.".into(),
        trigger_words: vec!["rewrite".into()],
    }];

    let cfg = EngineConfig {
        defaults,
        profiles: vec![profile],
        prompts,
        openai_api_key: llm_api_key,
        gemini_api_key: String::new(),
    };

    let engine = VoicewinEngine::new(
        cfg,
        Arc::new(DummyContextProvider),
        Arc::new(MockSttProvider),
        Arc::new(OpenAiCompatibleLlm),
        Arc::new(DummyInserter),
    );

    let audio = AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 16],
    };

    let result = engine.run_session(audio).await?;
    println!("stage={:?}", result.stage);
    println!("final={:?}", result.final_text);
    println!(
        "timings: t={:?}ms e={:?}ms",
        result.timings.transcription_ms, result.timings.enhancement_ms
    );

    Ok(())
}

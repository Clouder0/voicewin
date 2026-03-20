use std::sync::Arc;
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::{GlobalDefaults, PowerModeOverrides, PowerModeProfile};
use voicewin_core::types::{AppIdentity, InsertMode, ProfileId, PromptId};
use voicewin_engine::engine::{EngineConfig, VoicewinEngine};
use voicewin_engine::traits::AudioInput;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Windows GUI client placeholder.
    // This compiles on Linux; actual GUI + Windows platform code will be implemented under cfg(windows).

    let llm_api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
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
        llm_base_url: std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".into()),
        llm_model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-5.4".into()),
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

    let prompts = vec![PromptTemplate {
        id: PromptId::new(),
        title: "Rewrite".into(),
        mode: PromptMode::Enhancer,
        prompt_text: "Clean up grammar and punctuation.".into(),
        trigger_words: vec!["rewrite".into()],
    }];

    let profiles = vec![PowerModeProfile {
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
    }];

    // For now, GUI uses test platform implementations.
    let ctx_provider = voicewin_platform::test::TestContextProvider::new(
        AppIdentity::new().with_process_name("slack.exe"),
        Default::default(),
    )
    .boxed();

    let stt = Arc::new(voicewin_runtime::stt::MockSttProvider {
        text: "rewrite hello rewrite".into(),
    });
    let llm = Arc::new(
        voicewin_runtime::llm::OpenAiCompatibleLlmProvider::new().expect("build llm http client"),
    );
    let inserter = Arc::new(voicewin_platform::test::StdoutInserter);

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles,
            prompts,
            openai_api_key: llm_api_key,
            gemini_api_key: String::new(),
        },
        ctx_provider,
        stt,
        llm,
        inserter,
    );

    // Placeholder: in the real GUI, audio comes from mic recording.
    let audio = AudioInput {
        sample_rate_hz: 16_000,
        samples: vec![0.0; 16],
    };

    let res = engine.run_session(audio).await?;
    println!("GUI placeholder session: {:?}", res.final_text);

    Ok(())
}

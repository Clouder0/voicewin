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

    let defaults = GlobalDefaults {
        enable_enhancement: !llm_api_key.trim().is_empty(),
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "mock".into(),
        language: "en".into(),
        llm_base_url: std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".into()),
        llm_model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
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
    let llm = Arc::new(voicewin_runtime::llm::OpenAiCompatibleLlmProvider::new(
        llm_api_key.clone(),
    ));
    let inserter = Arc::new(voicewin_platform::test::StdoutInserter);

    let engine = VoicewinEngine::new(
        EngineConfig {
            defaults,
            profiles,
            prompts,
            llm_api_key,
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

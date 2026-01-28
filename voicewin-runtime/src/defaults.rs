use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::GlobalDefaults;
use voicewin_core::types::{InsertMode, PromptId};

pub fn default_prompt_templates() -> Vec<PromptTemplate> {
    vec![PromptTemplate {
        id: PromptId::new(),
        title: "Default".into(),
        mode: PromptMode::Enhancer,
        prompt_text:
            "Fix grammar, punctuation, and capitalization. Output only the corrected text.".into(),
        trigger_words: vec!["rewrite".into(), "clean up".into()],
    }]
}

pub fn default_global_defaults() -> GlobalDefaults {
    GlobalDefaults {
        // Default off: local dictation should work out-of-box without requiring
        // any cloud API keys.
        enable_enhancement: false,
        prompt_id: None,
        insert_mode: InsertMode::Paste,
        stt_provider: "local".into(),
        stt_model: "whisper".into(),
        language: "auto".into(),
        llm_base_url: "https://api.openai.com/v1".into(),
        llm_model: "gpt-4o-mini".into(),
        microphone_device: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    }
}

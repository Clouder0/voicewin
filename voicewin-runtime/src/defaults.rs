use voicewin_core::config::AppConfig;
use voicewin_core::enhancement::{PromptMode, PromptTemplate};
use voicewin_core::power_mode::GlobalDefaults;
use voicewin_core::types::{InsertMode, PromptId};

const LEGACY_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const LEGACY_OPENAI_MODEL: &str = "gpt-4o-mini";
const RECOMMENDED_OPENAI_MODEL: &str = "gpt-5.4";

pub fn default_prompt_templates() -> Vec<PromptTemplate> {
    vec![
        PromptTemplate {
            id: PromptId::new(),
            title: "Default Cleanup".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Fix grammar, punctuation, capitalization, and light dictation disfluencies while preserving the speaker's meaning. Output only the cleaned text.".into(),
            trigger_words: vec!["clean up".into()],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Rewrite".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Rewrite the transcript for clarity and flow while preserving the original meaning. Output only the rewritten text.".into(),
            trigger_words: vec!["rewrite".into()],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Email".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Turn the transcript into a polished email. Keep it concise, clear, and ready to send. Output only the email body.".into(),
            trigger_words: vec!["email".into()],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Chat Reply".into(),
            mode: PromptMode::Enhancer,
            prompt_text: "Turn the transcript into a natural chat-style message with clean punctuation and a concise, conversational tone. Output only the message.".into(),
            trigger_words: vec!["chat".into()],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Assistant".into(),
            mode: PromptMode::Assistant,
            prompt_text: "Follow the user's spoken instruction using the transcript and any provided context. Return only the requested final output.".into(),
            trigger_words: vec!["assistant".into()],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Edit Selection".into(),
            mode: PromptMode::Assistant,
            prompt_text: "Your only job is to rewrite CURRENTLY_SELECTED_TEXT. The transcript is the edit instruction, not the source text. CURRENTLY_SELECTED_TEXT is the text to rewrite. Never ask for the text again. If CURRENTLY_SELECTED_TEXT is present, rewrite that text according to the transcript and return only the rewritten text. If CURRENTLY_SELECTED_TEXT is missing, say briefly that no selected text is available.".into(),
            trigger_words: vec![],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Shorten Selection".into(),
            mode: PromptMode::Assistant,
            prompt_text: "Your only job is to rewrite CURRENTLY_SELECTED_TEXT so it is shorter and crisper while preserving meaning and important names. The transcript is an extra edit instruction, not the source text. CURRENTLY_SELECTED_TEXT is the text to rewrite. Never ask for the text again. If CURRENTLY_SELECTED_TEXT is present, rewrite that text accordingly and return only the rewritten selected text. If CURRENTLY_SELECTED_TEXT is missing, say briefly that no selected text is available.".into(),
            trigger_words: vec![],
        },
        PromptTemplate {
            id: PromptId::new(),
            title: "Explain Selection".into(),
            mode: PromptMode::Assistant,
            prompt_text: "Your only job is to answer or explain based on CURRENTLY_SELECTED_TEXT. The transcript is the user's question or instruction, not the source text. CURRENTLY_SELECTED_TEXT is the source material to analyze. Never ask for the text again. Return only the requested explanation, answer, or transformation. If CURRENTLY_SELECTED_TEXT is missing, say briefly that no selected text is available.".into(),
            trigger_words: vec![],
        },
    ]
}

pub fn backfill_default_prompts(cfg: &mut AppConfig) -> bool {
    let mut changed = false;
    let builtin_prompts = default_prompt_templates();

    if cfg.prompts.is_empty() {
        cfg.prompts = builtin_prompts;
        changed = true;
    } else {
        for builtin in builtin_prompts {
            if cfg
                .prompts
                .iter()
                .any(|prompt| prompt.title == builtin.title)
            {
                continue;
            }
            cfg.prompts.push(builtin);
            changed = true;
        }
    }

    let has_prompt = |prompt_id: &voicewin_core::types::PromptId| {
        cfg.prompts.iter().any(|prompt| &prompt.id == prompt_id)
    };

    if cfg
        .defaults
        .prompt_id
        .as_ref()
        .is_some_and(|prompt_id| !has_prompt(prompt_id))
    {
        cfg.defaults.prompt_id = None;
        changed = true;
    }

    for profile in &mut cfg.profiles {
        if profile
            .overrides
            .prompt_id
            .as_ref()
            .is_some_and(|prompt_id| !has_prompt(prompt_id))
        {
            profile.overrides.prompt_id = None;
            changed = true;
        }
    }

    changed
}

pub fn migrate_legacy_openai_defaults_to_recommended(cfg: &mut AppConfig) -> bool {
    if !matches_legacy_openai_default_stack(&cfg.defaults) {
        return false;
    }

    cfg.defaults.llm_model = RECOMMENDED_OPENAI_MODEL.into();
    cfg.defaults.llm_api_kind = "responses_sse".into();
    cfg.defaults.llm_preflight_mode = "off".into();
    cfg.defaults.llm_preflight_delay_ms = default_global_defaults().llm_preflight_delay_ms;
    cfg.defaults.llm_reasoning_effort = None;
    true
}

pub fn migrate_legacy_openai_profile_overrides_to_recommended(cfg: &mut AppConfig) -> bool {
    let mut changed = false;

    for profile in &mut cfg.profiles {
        if !matches_legacy_openai_profile_stack(&cfg.defaults, &profile.overrides) {
            continue;
        }

        profile.overrides.llm_base_url = Some(LEGACY_OPENAI_BASE_URL.into());
        profile.overrides.llm_model = Some(RECOMMENDED_OPENAI_MODEL.into());
        profile.overrides.llm_api_kind = Some("responses_sse".into());
        profile.overrides.llm_reasoning_effort = None;
        changed = true;
    }

    changed
}

fn matches_legacy_openai_default_stack(defaults: &GlobalDefaults) -> bool {
    defaults.llm_provider_kind == "openai_compatible"
        && normalize_base_url(&defaults.llm_base_url) == LEGACY_OPENAI_BASE_URL
        && defaults.llm_model == LEGACY_OPENAI_MODEL
        && defaults.llm_api_kind == "chat_completions"
        && defaults.llm_preflight_mode == "off"
        && defaults
            .llm_reasoning_effort
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
}

fn matches_legacy_openai_profile_stack(
    defaults: &GlobalDefaults,
    overrides: &voicewin_core::power_mode::PowerModeOverrides,
) -> bool {
    let resolved_provider_kind = overrides
        .llm_provider_kind
        .as_deref()
        .unwrap_or(&defaults.llm_provider_kind);

    resolved_provider_kind == "openai_compatible"
        && overrides
            .llm_base_url
            .as_deref()
            .is_some_and(|value| normalize_base_url(value) == LEGACY_OPENAI_BASE_URL)
        && overrides.llm_model.as_deref() == Some(LEGACY_OPENAI_MODEL)
        && overrides.llm_api_kind.as_deref() == Some("chat_completions")
        && overrides
            .llm_preflight_mode
            .as_deref()
            .is_none_or(|value| value == "off")
        && overrides
            .llm_reasoning_effort
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
}

fn normalize_base_url(value: &str) -> &str {
    value.trim().trim_end_matches('/')
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
        llm_provider_kind: "openai_compatible".into(),
        llm_base_url: LEGACY_OPENAI_BASE_URL.into(),
        llm_model: RECOMMENDED_OPENAI_MODEL.into(),
        llm_api_kind: "responses_sse".into(),
        llm_preflight_mode: "off".into(),
        llm_preflight_delay_ms: 1_500,
        screenshot_max_edge_px: 1_280,
        llm_reasoning_effort: None,
        microphone_device: None,
        microphone_device_id: None,
        history_enabled: true,
        context: voicewin_core::context::ContextToggles::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_core::config::AppConfig;
    use voicewin_core::power_mode::{PowerModeOverrides, PowerModeProfile};
    use voicewin_core::types::ProfileId;

    #[test]
    fn backfill_default_prompts_populates_empty_catalog() {
        let mut cfg = AppConfig {
            defaults: default_global_defaults(),
            profiles: vec![],
            prompts: vec![],
            llm_api_key_present: false,
        };

        let changed = backfill_default_prompts(&mut cfg);

        assert!(changed);
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Default Cleanup")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Edit Selection")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Shorten Selection")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Explain Selection")
        );
    }

    #[test]
    fn backfill_default_prompts_clears_missing_prompt_references() {
        let missing_prompt = PromptId::new();

        let mut cfg = AppConfig {
            defaults: GlobalDefaults {
                prompt_id: Some(missing_prompt.clone()),
                ..default_global_defaults()
            },
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "Editor".into(),
                enabled: true,
                matchers: vec![],
                overrides: PowerModeOverrides {
                    prompt_id: Some(missing_prompt),
                    ..PowerModeOverrides::default()
                },
            }],
            prompts: default_prompt_templates(),
            llm_api_key_present: false,
        };

        let changed = backfill_default_prompts(&mut cfg);

        assert!(changed);
        assert_eq!(cfg.defaults.prompt_id, None);
        assert_eq!(cfg.profiles[0].overrides.prompt_id, None);
    }

    #[test]
    fn backfill_default_prompts_appends_new_builtin_prompts_to_existing_catalog() {
        let mut cfg = AppConfig {
            defaults: default_global_defaults(),
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Team Custom".into(),
                mode: PromptMode::Assistant,
                prompt_text: "Custom prompt.".into(),
                trigger_words: vec![],
            }],
            llm_api_key_present: false,
        };

        let changed = backfill_default_prompts(&mut cfg);

        assert!(changed);
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Team Custom")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Edit Selection")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Shorten Selection")
        );
        assert!(
            cfg.prompts
                .iter()
                .any(|prompt| prompt.title == "Explain Selection")
        );

        let count_after_first_backfill = cfg.prompts.len();
        let changed_again = backfill_default_prompts(&mut cfg);
        assert!(!changed_again);
        assert_eq!(cfg.prompts.len(), count_after_first_backfill);
    }

    #[test]
    fn default_global_defaults_use_responses_and_gpt5_4() {
        let defaults = default_global_defaults();

        assert_eq!(defaults.llm_provider_kind, "openai_compatible");
        assert_eq!(defaults.llm_base_url, LEGACY_OPENAI_BASE_URL);
        assert_eq!(defaults.llm_model, RECOMMENDED_OPENAI_MODEL);
        assert_eq!(defaults.llm_api_kind, "responses_sse");
        assert_eq!(defaults.llm_preflight_mode, "off");
        assert_eq!(defaults.llm_preflight_delay_ms, 1_500);
        assert_eq!(defaults.screenshot_max_edge_px, 1_280);
        assert_eq!(defaults.llm_reasoning_effort, None);
    }

    #[test]
    fn migrate_legacy_openai_defaults_upgrades_exact_old_stack() {
        let mut cfg = AppConfig {
            defaults: GlobalDefaults {
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: LEGACY_OPENAI_BASE_URL.into(),
                llm_model: LEGACY_OPENAI_MODEL.into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                llm_reasoning_effort: None,
                ..default_global_defaults()
            },
            profiles: vec![],
            prompts: default_prompt_templates(),
            llm_api_key_present: false,
        };

        let changed = migrate_legacy_openai_defaults_to_recommended(&mut cfg);

        assert!(changed);
        assert_eq!(cfg.defaults.llm_model, RECOMMENDED_OPENAI_MODEL);
        assert_eq!(cfg.defaults.llm_api_kind, "responses_sse");
        assert_eq!(cfg.defaults.llm_preflight_mode, "off");
        assert_eq!(cfg.defaults.llm_preflight_delay_ms, 1_500);
        assert_eq!(cfg.defaults.llm_reasoning_effort, None);
    }

    #[test]
    fn migrate_legacy_openai_defaults_skips_customized_stack() {
        let mut cfg = AppConfig {
            defaults: GlobalDefaults {
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: LEGACY_OPENAI_BASE_URL.into(),
                llm_model: LEGACY_OPENAI_MODEL.into(),
                llm_api_kind: "chat_completions".into(),
                llm_preflight_mode: "http_connect".into(),
                llm_preflight_delay_ms: 1_500,
                llm_reasoning_effort: None,
                ..default_global_defaults()
            },
            profiles: vec![],
            prompts: default_prompt_templates(),
            llm_api_key_present: false,
        };

        let changed = migrate_legacy_openai_defaults_to_recommended(&mut cfg);

        assert!(!changed);
        assert_eq!(cfg.defaults.llm_model, LEGACY_OPENAI_MODEL);
        assert_eq!(cfg.defaults.llm_api_kind, "chat_completions");
        assert_eq!(cfg.defaults.llm_preflight_mode, "http_connect");
        assert_eq!(cfg.defaults.llm_preflight_delay_ms, 1_500);
    }

    #[test]
    fn migrate_legacy_openai_profile_overrides_upgrades_exact_old_profile_stack() {
        let mut cfg = AppConfig {
            defaults: default_global_defaults(),
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "Editor".into(),
                enabled: true,
                matchers: vec![],
                overrides: PowerModeOverrides {
                    llm_provider_kind: Some("openai_compatible".into()),
                    llm_base_url: Some(LEGACY_OPENAI_BASE_URL.into()),
                    llm_model: Some(LEGACY_OPENAI_MODEL.into()),
                    llm_api_kind: Some("chat_completions".into()),
                    llm_preflight_mode: None,
                    llm_reasoning_effort: None,
                    ..PowerModeOverrides::default()
                },
            }],
            prompts: default_prompt_templates(),
            llm_api_key_present: false,
        };

        let changed = migrate_legacy_openai_profile_overrides_to_recommended(&mut cfg);

        assert!(changed);
        assert_eq!(
            cfg.profiles[0].overrides.llm_model.as_deref(),
            Some(RECOMMENDED_OPENAI_MODEL)
        );
        assert_eq!(
            cfg.profiles[0].overrides.llm_api_kind.as_deref(),
            Some("responses_sse")
        );
        assert_eq!(
            cfg.profiles[0].overrides.llm_base_url.as_deref(),
            Some(LEGACY_OPENAI_BASE_URL)
        );
    }

    #[test]
    fn migrate_legacy_openai_profile_overrides_skips_customized_or_non_openai_profiles() {
        let mut cfg = AppConfig {
            defaults: GlobalDefaults {
                llm_provider_kind: "gemini".into(),
                ..default_global_defaults()
            },
            profiles: vec![
                PowerModeProfile {
                    id: ProfileId::new(),
                    name: "Customized".into(),
                    enabled: true,
                    matchers: vec![],
                    overrides: PowerModeOverrides {
                        llm_provider_kind: Some("openai_compatible".into()),
                        llm_base_url: Some(LEGACY_OPENAI_BASE_URL.into()),
                        llm_model: Some(LEGACY_OPENAI_MODEL.into()),
                        llm_api_kind: Some("chat_completions".into()),
                        llm_preflight_mode: Some("http_connect".into()),
                        ..PowerModeOverrides::default()
                    },
                },
                PowerModeProfile {
                    id: ProfileId::new(),
                    name: "Inherited Gemini".into(),
                    enabled: true,
                    matchers: vec![],
                    overrides: PowerModeOverrides {
                        llm_base_url: Some(LEGACY_OPENAI_BASE_URL.into()),
                        llm_model: Some(LEGACY_OPENAI_MODEL.into()),
                        llm_api_kind: Some("chat_completions".into()),
                        ..PowerModeOverrides::default()
                    },
                },
            ],
            prompts: default_prompt_templates(),
            llm_api_key_present: false,
        };

        let changed = migrate_legacy_openai_profile_overrides_to_recommended(&mut cfg);

        assert!(!changed);
        assert_eq!(
            cfg.profiles[0].overrides.llm_model.as_deref(),
            Some(LEGACY_OPENAI_MODEL)
        );
        assert_eq!(
            cfg.profiles[1].overrides.llm_api_kind.as_deref(),
            Some("chat_completions")
        );
    }
}

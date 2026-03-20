use crate::types::{AppIdentity, InsertMode, ProfileId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppMatcher {
    ExePathEquals(String),
    ProcessNameEquals(String),
    WindowTitleContains(String),
}

impl AppMatcher {
    pub fn matches(&self, app: &AppIdentity) -> bool {
        match self {
            AppMatcher::ExePathEquals(expected) => app
                .exe_path
                .as_ref()
                .is_some_and(|p| normalize(p.as_str()) == normalize(expected)),
            AppMatcher::ProcessNameEquals(expected) => app
                .process_name
                .as_ref()
                .is_some_and(|p| normalize(&p.0) == normalize(expected)),
            AppMatcher::WindowTitleContains(needle) => app
                .window_title
                .as_ref()
                .is_some_and(|t| normalize(&t.0).contains(&normalize(needle))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PowerModeOverrides {
    pub enable_enhancement: Option<bool>,
    pub prompt_id: Option<crate::types::PromptId>,
    pub insert_mode: Option<InsertMode>,
    pub stt_provider: Option<String>,
    pub stt_model: Option<String>,
    pub language: Option<String>,
    pub llm_provider_kind: Option<String>,
    pub llm_base_url: Option<String>,
    pub llm_model: Option<String>,
    pub llm_api_kind: Option<String>,
    pub llm_preflight_mode: Option<String>,
    pub llm_reasoning_effort: Option<String>,

    // Context toggles (best-effort on Windows)
    pub context: Option<crate::context::ContextToggles>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerModeProfile {
    pub id: ProfileId,
    pub name: String,
    pub enabled: bool,
    pub matchers: Vec<AppMatcher>,
    pub overrides: PowerModeOverrides,
}

impl PowerModeProfile {
    pub fn matches(&self, app: &AppIdentity) -> bool {
        if !self.enabled {
            return false;
        }

        // Minimal & predictable: if any matcher matches, profile matches.
        self.matchers.iter().any(|m| m.matches(app))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalDefaults {
    pub enable_enhancement: bool,
    pub prompt_id: Option<crate::types::PromptId>,
    pub insert_mode: InsertMode,
    pub stt_provider: String,
    pub stt_model: String,
    pub language: String,
    #[serde(default = "default_llm_provider_kind")]
    pub llm_provider_kind: String,
    pub llm_base_url: String,
    pub llm_model: String,
    #[serde(default = "default_llm_api_kind")]
    pub llm_api_kind: String,
    #[serde(default = "default_llm_preflight_mode")]
    pub llm_preflight_mode: String,
    #[serde(default = "default_llm_preflight_delay_ms")]
    pub llm_preflight_delay_ms: u64,
    #[serde(default = "default_screenshot_max_edge_px")]
    pub screenshot_max_edge_px: u32,
    #[serde(default)]
    pub llm_reasoning_effort: Option<String>,

    /// Optional preferred microphone device name.
    ///
    /// When `None`, the system default input device is used.
    #[serde(default)]
    pub microphone_device: Option<String>,

    /// Optional preferred microphone device id.
    ///
    /// When present, this takes precedence over `microphone_device` because names can be
    /// duplicated across different hardware devices.
    #[serde(default)]
    pub microphone_device_id: Option<String>,

    #[serde(default = "default_history_enabled")]
    pub history_enabled: bool,

    pub context: crate::context::ContextToggles,
}

fn default_history_enabled() -> bool {
    // History is always enabled in the design-draft UI.
    // Keeping a default here ensures older configs remain valid.
    true
}

fn default_llm_provider_kind() -> String {
    "openai_compatible".into()
}

fn default_llm_api_kind() -> String {
    "chat_completions".into()
}

fn default_llm_preflight_mode() -> String {
    "off".into()
}

fn default_llm_preflight_delay_ms() -> u64 {
    1_500
}

fn default_screenshot_max_edge_px() -> u32 {
    1_280
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveConfig {
    pub enable_enhancement: bool,
    pub prompt_id: Option<crate::types::PromptId>,
    pub insert_mode: InsertMode,
    pub stt_provider: String,
    pub stt_model: String,
    pub language: String,
    #[serde(default = "default_llm_provider_kind")]
    pub llm_provider_kind: String,
    pub llm_base_url: String,
    pub llm_model: String,
    #[serde(default = "default_llm_api_kind")]
    pub llm_api_kind: String,
    #[serde(default = "default_llm_preflight_mode")]
    pub llm_preflight_mode: String,
    #[serde(default = "default_llm_preflight_delay_ms")]
    pub llm_preflight_delay_ms: u64,
    #[serde(default = "default_screenshot_max_edge_px")]
    pub screenshot_max_edge_px: u32,
    #[serde(default)]
    pub llm_reasoning_effort: Option<String>,

    pub context: crate::context::ContextToggles,

    // The active profile resolved for the current foreground app.
    pub matched_profile_id: Option<ProfileId>,

    // The resolved profile name for UI display.
    #[serde(default)]
    pub matched_profile_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EphemeralOverrides {
    pub forced_profile_id: Option<ProfileId>,
    pub forced_prompt_id: Option<crate::types::PromptId>,
    pub forced_enable_enhancement: Option<bool>,
}

pub fn resolve_effective_config(
    defaults: &GlobalDefaults,
    profiles: &[PowerModeProfile],
    app: &AppIdentity,
    ephemeral: &EphemeralOverrides,
) -> EffectiveConfig {
    // 1) Determine which profile matches.
    let matched_profile = if let Some(forced_id) = &ephemeral.forced_profile_id {
        profiles
            .iter()
            .find(|p| &p.id == forced_id)
            .filter(|p| p.enabled)
    } else {
        profiles.iter().find(|p| p.matches(app))
    };

    let mut cfg = EffectiveConfig {
        enable_enhancement: defaults.enable_enhancement,
        prompt_id: defaults.prompt_id.clone(),
        insert_mode: defaults.insert_mode,
        stt_provider: defaults.stt_provider.clone(),
        stt_model: defaults.stt_model.clone(),
        language: defaults.language.clone(),
        llm_provider_kind: defaults.llm_provider_kind.clone(),
        llm_base_url: defaults.llm_base_url.clone(),
        llm_model: defaults.llm_model.clone(),
        llm_api_kind: defaults.llm_api_kind.clone(),
        llm_preflight_mode: defaults.llm_preflight_mode.clone(),
        llm_preflight_delay_ms: defaults.llm_preflight_delay_ms,
        screenshot_max_edge_px: defaults.screenshot_max_edge_px,
        llm_reasoning_effort: defaults.llm_reasoning_effort.clone(),
        context: defaults.context.clone(),
        matched_profile_id: matched_profile.map(|p| p.id.clone()),
        matched_profile_name: matched_profile.map(|p| p.name.clone()),
    };

    // 2) Apply profile overrides.
    if let Some(profile) = matched_profile {
        apply_overrides(&mut cfg, &profile.overrides);
    }

    // 3) Apply ephemeral overrides.
    if let Some(enabled) = ephemeral.forced_enable_enhancement {
        cfg.enable_enhancement = enabled;
    }
    if let Some(prompt_id) = &ephemeral.forced_prompt_id {
        cfg.prompt_id = Some(prompt_id.clone());
        cfg.enable_enhancement = true; // selecting a prompt implies enhancement.
    }

    cfg
}

fn apply_overrides(cfg: &mut EffectiveConfig, overrides: &PowerModeOverrides) {
    if let Some(v) = overrides.enable_enhancement {
        cfg.enable_enhancement = v;
    }
    if let Some(v) = &overrides.prompt_id {
        cfg.prompt_id = Some(v.clone());
    }
    if let Some(v) = overrides.insert_mode {
        cfg.insert_mode = v;
    }
    if let Some(v) = &overrides.stt_provider {
        cfg.stt_provider = v.clone();
    }
    if let Some(v) = &overrides.stt_model {
        cfg.stt_model = v.clone();
    }
    if let Some(v) = &overrides.language {
        cfg.language = v.clone();
    }
    if let Some(v) = &overrides.llm_provider_kind {
        cfg.llm_provider_kind = v.clone();
    }
    if let Some(v) = &overrides.llm_base_url {
        cfg.llm_base_url = v.clone();
    }
    if let Some(v) = &overrides.llm_model {
        cfg.llm_model = v.clone();
    }
    if let Some(v) = &overrides.llm_api_kind {
        cfg.llm_api_kind = v.clone();
    }
    if let Some(v) = &overrides.llm_preflight_mode {
        cfg.llm_preflight_mode = v.clone();
    }
    if let Some(v) = &overrides.llm_reasoning_effort {
        cfg.llm_reasoning_effort = Some(v.clone());
    }
    if let Some(v) = &overrides.context {
        cfg.context = v.clone();
    }
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AppIdentity;

    #[test]
    fn matcher_exe_path_equals_is_case_insensitive() {
        let app = AppIdentity::new().with_exe_path("C:\\Program Files\\Slack\\slack.exe");
        let m = AppMatcher::ExePathEquals("c:\\program files\\slack\\SLACK.EXE".into());
        assert!(m.matches(&app));
    }

    #[test]
    fn matcher_window_title_contains_is_case_insensitive() {
        let app = AppIdentity::new().with_window_title("GitHub - Pull Requests");
        let m = AppMatcher::WindowTitleContains("pull".into());
        assert!(m.matches(&app));
    }

    #[test]
    fn resolve_uses_forced_profile_when_present() {
        let defaults = GlobalDefaults {
            enable_enhancement: false,
            prompt_id: None,
            insert_mode: crate::types::InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "whisper".into(),
            language: "en".into(),
            llm_provider_kind: default_llm_provider_kind(),
            llm_base_url: "http://localhost".into(),
            llm_model: "gpt-4o-mini".into(),
            llm_api_kind: default_llm_api_kind(),
            llm_preflight_mode: default_llm_preflight_mode(),
            llm_preflight_delay_ms: default_llm_preflight_delay_ms(),
            screenshot_max_edge_px: default_screenshot_max_edge_px(),
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: crate::context::ContextToggles::default(),
        };

        let p1 = PowerModeProfile {
            id: ProfileId::new(),
            name: "Slack".into(),
            enabled: true,
            matchers: vec![AppMatcher::ProcessNameEquals("slack.exe".into())],
            overrides: PowerModeOverrides {
                enable_enhancement: Some(true),
                ..Default::default()
            },
        };

        let p2_id = ProfileId::new();
        let p2 = PowerModeProfile {
            id: p2_id.clone(),
            name: "VS Code".into(),
            enabled: true,
            matchers: vec![AppMatcher::ProcessNameEquals("code.exe".into())],
            overrides: PowerModeOverrides {
                enable_enhancement: Some(false),
                ..Default::default()
            },
        };

        let app = AppIdentity::new().with_process_name("slack.exe");

        let cfg = resolve_effective_config(
            &defaults,
            &[p1, p2],
            &app,
            &EphemeralOverrides {
                forced_profile_id: Some(p2_id),
                ..Default::default()
            },
        );

        assert_eq!(cfg.enable_enhancement, false);
    }

    #[test]
    fn resolve_applies_llm_preflight_mode_override() {
        let defaults = GlobalDefaults {
            enable_enhancement: false,
            prompt_id: None,
            insert_mode: crate::types::InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "whisper".into(),
            language: "en".into(),
            llm_provider_kind: default_llm_provider_kind(),
            llm_base_url: "http://localhost".into(),
            llm_model: "gpt-4o-mini".into(),
            llm_api_kind: default_llm_api_kind(),
            llm_preflight_mode: default_llm_preflight_mode(),
            llm_preflight_delay_ms: default_llm_preflight_delay_ms(),
            screenshot_max_edge_px: default_screenshot_max_edge_px(),
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: crate::context::ContextToggles::default(),
        };

        let profile = PowerModeProfile {
            id: ProfileId::new(),
            name: "Slack".into(),
            enabled: true,
            matchers: vec![AppMatcher::ProcessNameEquals("slack.exe".into())],
            overrides: PowerModeOverrides {
                llm_preflight_mode: Some("off".into()),
                ..Default::default()
            },
        };

        let app = AppIdentity::new().with_process_name("slack.exe");
        let cfg =
            resolve_effective_config(&defaults, &[profile], &app, &EphemeralOverrides::default());

        assert_eq!(cfg.llm_preflight_mode, "off");
    }
}

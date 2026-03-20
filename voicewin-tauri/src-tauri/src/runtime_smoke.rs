#![allow(dead_code)]

use voicewin_core::config::AppConfig;
use voicewin_core::power_mode::GlobalDefaults;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeSmokeMode {
    pub(crate) transcript: String,
    pub(crate) expected_foreground_process: Option<String>,
    pub(crate) start_marker: String,
    pub(crate) success_marker: String,
    version: String,
    git_sha: String,
}

pub(crate) const RUNTIME_SMOKE_ENABLE_ENV: &str = "VOICEWIN_RUNTIME_SMOKE_TEST";
pub(crate) const RUNTIME_SMOKE_TRANSCRIPT_ENV: &str = "VOICEWIN_RUNTIME_SMOKE_TRANSCRIPT";
pub(crate) const RUNTIME_SMOKE_EXPECT_PROCESS_ENV: &str = "VOICEWIN_RUNTIME_SMOKE_EXPECT_PROCESS";
pub(crate) const RUNTIME_SMOKE_REFOCUS_DELAY: std::time::Duration =
    std::time::Duration::from_millis(300);
pub(crate) const RUNTIME_SMOKE_FOREGROUND_MATCH_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2);
pub(crate) const RUNTIME_SMOKE_FOREGROUND_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(100);
pub(crate) const RUNTIME_SMOKE_STAGE_REFOCUS_DELAY: &str = "refocus_delay";
pub(crate) const RUNTIME_SMOKE_STAGE_BUILD_SERVICE: &str = "build_service";
pub(crate) const RUNTIME_SMOKE_STAGE_CAPTURE_FOREGROUND: &str = "capture_foreground";
pub(crate) const RUNTIME_SMOKE_STAGE_SESSION: &str = "session";

pub(crate) fn runtime_smoke_enabled(input: Option<&str>) -> bool {
    input == Some("1")
}

pub(crate) fn runtime_smoke_start_marker(version: &str, git_sha: &str) -> String {
    format!("VOICEWIN_RUNTIME_SMOKE_START version={version} git_sha={git_sha}")
}

pub(crate) fn runtime_smoke_stage_marker(stage: &str) -> String {
    format!("VOICEWIN_RUNTIME_SMOKE_STAGE stage={stage}")
}

pub(crate) fn runtime_smoke_success_marker(version: &str, git_sha: &str) -> String {
    format!("VOICEWIN_RUNTIME_SMOKE_OK version={version} git_sha={git_sha}")
}

pub(crate) fn runtime_smoke_failure_marker(version: &str, git_sha: &str, reason: &str) -> String {
    format!("VOICEWIN_RUNTIME_SMOKE_FAIL version={version} git_sha={git_sha} reason={reason}")
}

pub(crate) fn write_runtime_smoke_process_output<W: std::io::Write>(
    writer: &mut W,
    marker: &str,
) -> std::io::Result<()> {
    writeln!(writer, "{marker}")
}

pub(crate) fn write_runtime_smoke_start_process_output<W: std::io::Write>(
    writer: &mut W,
    mode: &RuntimeSmokeMode,
    version: &str,
    git_sha: &str,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "{}",
        crate::startup_smoke::startup_provenance_log(version, git_sha)
    )?;
    write_runtime_smoke_process_output(writer, &mode.start_marker)
}

pub(crate) fn parse_runtime_smoke_env_value(input: Option<&str>) -> Option<String> {
    input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn foreground_process_matches(
    actual_process_name: Option<&str>,
    expected_process_name: Option<&str>,
) -> bool {
    match parse_runtime_smoke_env_value(expected_process_name) {
        None => true,
        Some(expected) => parse_runtime_smoke_env_value(actual_process_name)
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected)),
    }
}

pub(crate) fn runtime_smoke_mode(
    enabled: Option<&str>,
    transcript: Option<&str>,
    expected_process: Option<&str>,
    version: &str,
    git_sha: &str,
) -> Result<Option<RuntimeSmokeMode>, String> {
    if !runtime_smoke_enabled(enabled) {
        return Ok(None);
    }

    let transcript = parse_runtime_smoke_env_value(transcript).ok_or_else(|| {
        format!(
            "{} must be set to a non-empty transcript",
            RUNTIME_SMOKE_TRANSCRIPT_ENV
        )
    })?;

    Ok(Some(RuntimeSmokeMode {
        transcript,
        expected_foreground_process: parse_runtime_smoke_env_value(expected_process),
        start_marker: runtime_smoke_start_marker(version, git_sha),
        success_marker: runtime_smoke_success_marker(version, git_sha),
        version: version.to_owned(),
        git_sha: git_sha.to_owned(),
    }))
}

pub(crate) fn deterministic_runtime_smoke_config(
    mut cfg: AppConfig,
    smoke_defaults: GlobalDefaults,
) -> AppConfig {
    // Keep packaged runtime smoke deterministic by stripping user-defined
    // profile/prompt behavior and forcing a known-good default session config.
    cfg.defaults = smoke_defaults;
    cfg.profiles.clear();
    cfg.prompts.clear();
    cfg
}

impl RuntimeSmokeMode {
    pub(crate) fn stage_marker(&self, stage: &str) -> String {
        runtime_smoke_stage_marker(stage)
    }

    pub(crate) fn failure_marker(&self, reason: &str) -> String {
        runtime_smoke_failure_marker(&self.version, &self.git_sha, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_core::enhancement::{PromptMode, PromptTemplate};
    use voicewin_core::power_mode::{
        AppMatcher, GlobalDefaults, PowerModeOverrides, PowerModeProfile,
    };
    use voicewin_core::types::{InsertMode, ProfileId, PromptId};

    #[test]
    fn runtime_smoke_is_disabled_by_default() {
        assert_eq!(
            runtime_smoke_mode(None, None, None, "0.1.4", "abc123"),
            Ok(None)
        );
    }

    #[test]
    fn runtime_smoke_requires_exact_enable_flag_and_non_empty_transcript() {
        assert_eq!(
            runtime_smoke_mode(Some("1 "), Some("hello"), None, "0.1.4", "abc123"),
            Ok(None)
        );
        assert!(runtime_smoke_mode(Some("1"), Some("   "), None, "0.1.4", "abc123").is_err());
    }

    #[test]
    fn runtime_smoke_builds_expected_markers() {
        let mode = runtime_smoke_mode(
            Some("1"),
            Some("hello world"),
            Some("TextEdit"),
            "0.1.4",
            "abc123",
        )
        .unwrap()
        .unwrap();

        assert_eq!(mode.transcript, "hello world");
        assert_eq!(
            mode.expected_foreground_process.as_deref(),
            Some("TextEdit")
        );
        assert_eq!(
            mode.start_marker,
            "VOICEWIN_RUNTIME_SMOKE_START version=0.1.4 git_sha=abc123"
        );
        assert_eq!(
            mode.stage_marker("session"),
            "VOICEWIN_RUNTIME_SMOKE_STAGE stage=session"
        );
        assert_eq!(
            mode.success_marker,
            "VOICEWIN_RUNTIME_SMOKE_OK version=0.1.4 git_sha=abc123"
        );
        assert_eq!(
            mode.failure_marker("foreground_mismatch"),
            "VOICEWIN_RUNTIME_SMOKE_FAIL version=0.1.4 git_sha=abc123 reason=foreground_mismatch"
        );
    }

    #[test]
    fn runtime_smoke_expected_process_matching_is_case_insensitive() {
        assert!(foreground_process_matches(
            Some("notepad.exe"),
            Some("NOTEPAD.EXE")
        ));
        assert!(!foreground_process_matches(
            Some("TextEdit"),
            Some("Notepad")
        ));
    }

    #[test]
    fn runtime_smoke_helper_builds_markers_without_mode() {
        assert_eq!(
            runtime_smoke_start_marker("0.1.4", "abc123"),
            "VOICEWIN_RUNTIME_SMOKE_START version=0.1.4 git_sha=abc123"
        );
        assert_eq!(
            runtime_smoke_stage_marker("capture_foreground"),
            "VOICEWIN_RUNTIME_SMOKE_STAGE stage=capture_foreground"
        );
        assert_eq!(
            runtime_smoke_success_marker("0.1.4", "abc123"),
            "VOICEWIN_RUNTIME_SMOKE_OK version=0.1.4 git_sha=abc123"
        );
        assert_eq!(
            runtime_smoke_failure_marker("0.1.4", "abc123", "foreground_mismatch"),
            "VOICEWIN_RUNTIME_SMOKE_FAIL version=0.1.4 git_sha=abc123 reason=foreground_mismatch"
        );
    }

    #[test]
    fn runtime_smoke_process_output_writes_one_marker_per_line() {
        let mut output = Vec::new();

        write_runtime_smoke_process_output(
            &mut output,
            "VOICEWIN_RUNTIME_SMOKE_STAGE stage=session",
        )
        .unwrap();
        write_runtime_smoke_process_output(
            &mut output,
            "VOICEWIN_RUNTIME_SMOKE_OK version=0.1.4 git_sha=abc123",
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "VOICEWIN_RUNTIME_SMOKE_STAGE stage=session\nVOICEWIN_RUNTIME_SMOKE_OK version=0.1.4 git_sha=abc123\n"
        );
    }

    #[test]
    fn runtime_smoke_start_process_output_writes_provenance_before_start_marker() {
        let mode = runtime_smoke_mode(Some("1"), Some("hello world"), None, "0.1.4", "abc123")
            .unwrap()
            .unwrap();
        let mut output = Vec::new();

        write_runtime_smoke_start_process_output(&mut output, &mode, "0.1.4", "abc123").unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "VoiceWin startup: version=0.1.4 git_sha=abc123\nVOICEWIN_RUNTIME_SMOKE_START version=0.1.4 git_sha=abc123\n"
        );
    }

    #[test]
    fn runtime_smoke_start_process_output_writes_provenance_then_start_marker() {
        let mode = runtime_smoke_mode(Some("1"), Some("hello world"), None, "0.1.4", "abc123")
            .unwrap()
            .unwrap();
        let mut output = Vec::new();

        write_runtime_smoke_start_process_output(&mut output, &mode, "0.1.4", "abc123").unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "VoiceWin startup: version=0.1.4 git_sha=abc123\nVOICEWIN_RUNTIME_SMOKE_START version=0.1.4 git_sha=abc123\n"
        );
    }

    #[test]
    fn runtime_smoke_refocus_delay_is_short_and_fixed() {
        assert_eq!(RUNTIME_SMOKE_REFOCUS_DELAY.as_millis(), 300);
    }

    #[test]
    fn runtime_smoke_foreground_retry_budget_is_bounded_and_non_zero() {
        assert_eq!(RUNTIME_SMOKE_FOREGROUND_MATCH_TIMEOUT.as_secs(), 2);
        assert_eq!(RUNTIME_SMOKE_FOREGROUND_POLL_INTERVAL.as_millis(), 100);
    }

    #[test]
    fn deterministic_runtime_smoke_config_uses_smoke_defaults_and_ignores_user_profiles_and_prompts(
    ) {
        let smoke_defaults = GlobalDefaults {
            enable_enhancement: false,
            prompt_id: None,
            insert_mode: InsertMode::Paste,
            stt_provider: "local".into(),
            stt_model: "/tmp/bootstrap.bin".into(),
            language: "auto".into(),
            llm_provider_kind: "openai_compatible".into(),
            llm_base_url: "https://api.openai.com/v1".into(),
            llm_model: "gpt-5.4".into(),
            llm_api_kind: "responses_sse".into(),
            llm_preflight_mode: "off".into(),
            llm_preflight_delay_ms: 1_500,
            screenshot_max_edge_px: 1_280,
            llm_reasoning_effort: None,
            microphone_device: None,
            microphone_device_id: None,
            history_enabled: true,
            context: voicewin_core::context::ContextToggles::default(),
        };

        let user_cfg = voicewin_core::config::AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: Some(PromptId::new()),
                insert_mode: InsertMode::PasteAndEnter,
                stt_provider: "elevenlabs".into(),
                stt_model: "scribe-v1".into(),
                language: "en".into(),
                llm_provider_kind: "openai_compatible".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "custom-model".into(),
                llm_api_kind: "responses_sse".into(),
                llm_preflight_mode: "off".into(),
                llm_preflight_delay_ms: 1_500,
                screenshot_max_edge_px: 1_280,
                llm_reasoning_effort: Some("high".into()),
                microphone_device: Some("USB Mic".into()),
                microphone_device_id: Some("cpal:1".into()),
                history_enabled: false,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![PowerModeProfile {
                id: ProfileId::new(),
                name: "User profile".into(),
                enabled: true,
                matchers: vec![AppMatcher::ProcessNameEquals("TextEdit".into())],
                overrides: PowerModeOverrides {
                    enable_enhancement: Some(true),
                    prompt_id: Some(PromptId::new()),
                    insert_mode: Some(InsertMode::ShiftInsert),
                    stt_provider: Some("elevenlabs".into()),
                    stt_model: Some("scribe-v1".into()),
                    language: Some("en".into()),
                    llm_provider_kind: Some("openai_compatible".into()),
                    llm_base_url: Some("https://example.com/v1".into()),
                    llm_model: Some("custom-model".into()),
                    llm_api_kind: Some("responses_sse".into()),
                    llm_preflight_mode: Some("off".into()),
                    llm_reasoning_effort: Some("medium".into()),
                    context: None,
                },
            }],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Rewrite".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Rewrite this".into(),
                trigger_words: vec!["rewrite".into()],
            }],
            llm_api_key_present: true,
        };

        let smoke_cfg = deterministic_runtime_smoke_config(user_cfg, smoke_defaults.clone());

        assert_eq!(smoke_cfg.defaults, smoke_defaults);
        assert!(smoke_cfg.profiles.is_empty());
        assert!(smoke_cfg.prompts.is_empty());
        assert!(smoke_cfg.llm_api_key_present);
    }
}

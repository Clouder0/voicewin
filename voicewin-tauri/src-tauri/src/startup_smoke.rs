#![cfg_attr(not(test), allow(dead_code))]

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupSmokeMode {
    pub(crate) marker: String,
}

pub(crate) const STARTUP_SMOKE_ENABLE_ENV: &str = "VOICEWIN_SMOKE_TEST";

pub(crate) fn startup_provenance_log(version: &str, git_sha: &str) -> String {
    format!("VoiceWin startup: version={version} git_sha={git_sha}")
}

pub(crate) fn write_startup_smoke_process_output<W: std::io::Write>(
    writer: &mut W,
    mode: &StartupSmokeMode,
    version: &str,
    git_sha: &str,
) -> std::io::Result<()> {
    writeln!(writer, "{}", startup_provenance_log(version, git_sha))?;
    writeln!(writer, "{}", mode.marker)?;
    Ok(())
}

pub(crate) fn startup_smoke_mode(
    enabled: Option<&str>,
    version: &str,
    git_sha: &str,
) -> Option<StartupSmokeMode> {
    if enabled != Some("1") {
        return None;
    }

    Some(StartupSmokeMode {
        marker: format!("VOICEWIN_SMOKE_OK version={version} git_sha={git_sha}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_mode_is_disabled_by_default() {
        assert_eq!(startup_smoke_mode(None, "0.1.4", "abc123"), None);
    }

    #[test]
    fn smoke_mode_builds_expected_marker_when_enabled() {
        let mode = startup_smoke_mode(Some("1"), "0.1.4", "abc123").unwrap();
        assert_eq!(
            mode.marker,
            "VOICEWIN_SMOKE_OK version=0.1.4 git_sha=abc123"
        );
    }

    #[test]
    fn startup_smoke_requires_exact_enable_flag() {
        assert!(startup_smoke_mode(Some("0"), "0.1.4", "abc123").is_none());
        assert!(startup_smoke_mode(Some("1 "), "0.1.4", "abc123").is_none());
        assert!(startup_smoke_mode(Some("1"), "0.1.4", "abc123").is_some());
    }

    #[test]
    fn startup_smoke_uses_packaged_app_env_flag() {
        assert_eq!(STARTUP_SMOKE_ENABLE_ENV, "VOICEWIN_SMOKE_TEST");
    }

    #[test]
    fn startup_provenance_log_matches_entrypoint_format() {
        assert_eq!(
            startup_provenance_log("0.1.4", "abc123"),
            "VoiceWin startup: version=0.1.4 git_sha=abc123"
        );
    }

    #[test]
    fn smoke_process_output_writes_provenance_then_marker() {
        let mode = startup_smoke_mode(Some("1"), "0.1.4", "abc123").unwrap();
        let mut output = Vec::new();

        write_startup_smoke_process_output(&mut output, &mode, "0.1.4", "abc123").unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "VoiceWin startup: version=0.1.4 git_sha=abc123\nVOICEWIN_SMOKE_OK version=0.1.4 git_sha=abc123\n"
        );
    }
}

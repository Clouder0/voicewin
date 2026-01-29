// Small helpers/constants for interpreting STT selections in config.

pub const STT_PROVIDER_LOCAL: &str = "local";
pub const STT_PROVIDER_ELEVENLABS: &str = "elevenlabs";

// ElevenLabs model selectors as exposed by VoiceWin.
//
// Note: `ELEVENLABS_MODEL_SCRIBE_V2_REALTIME` is an app-level selector; ElevenLabs uses
// `scribe_v2` for both batch and realtime model IDs.
pub const ELEVENLABS_MODEL_SCRIBE_V2: &str = "scribe_v2";
pub const ELEVENLABS_MODEL_SCRIBE_V2_REALTIME: &str = "scribe_v2_realtime";

pub fn is_elevenlabs_realtime_selected(provider: &str, model: &str) -> bool {
    provider == STT_PROVIDER_ELEVENLABS && model == ELEVENLABS_MODEL_SCRIBE_V2_REALTIME
}

pub fn normalize_elevenlabs_batch_model(model: &str) -> &str {
    // VoiceWin exposes `scribe_v2_realtime` as a separate selection, but the batch HTTP endpoint
    // expects `scribe_v2`.
    if model == ELEVENLABS_MODEL_SCRIBE_V2_REALTIME {
        ELEVENLABS_MODEL_SCRIBE_V2
    } else {
        model
    }
}

pub fn accept_transcript_override(text: String) -> Option<String> {
    // Used for realtime streaming: accept a transcript override only if it contains
    // non-whitespace content.
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_elevenlabs_realtime_selection() {
        assert!(is_elevenlabs_realtime_selected(
            STT_PROVIDER_ELEVENLABS,
            ELEVENLABS_MODEL_SCRIBE_V2_REALTIME
        ));
        assert!(!is_elevenlabs_realtime_selected(
            STT_PROVIDER_ELEVENLABS,
            ELEVENLABS_MODEL_SCRIBE_V2
        ));
        assert!(!is_elevenlabs_realtime_selected(
            STT_PROVIDER_LOCAL,
            ELEVENLABS_MODEL_SCRIBE_V2_REALTIME
        ));
    }

    #[test]
    fn normalizes_elevenlabs_batch_model() {
        assert_eq!(
            normalize_elevenlabs_batch_model(ELEVENLABS_MODEL_SCRIBE_V2_REALTIME),
            ELEVENLABS_MODEL_SCRIBE_V2
        );
        assert_eq!(
            normalize_elevenlabs_batch_model(ELEVENLABS_MODEL_SCRIBE_V2),
            ELEVENLABS_MODEL_SCRIBE_V2
        );
        assert_eq!(normalize_elevenlabs_batch_model("other"), "other");
    }

    #[test]
    fn transcript_override_accepts_only_non_empty() {
        assert_eq!(accept_transcript_override("".to_string()), None);
        assert_eq!(accept_transcript_override("   \n\t".to_string()), None);
        assert_eq!(
            accept_transcript_override(" hello ".to_string()),
            Some(" hello ".to_string())
        );
    }
}

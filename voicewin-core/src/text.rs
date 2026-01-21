use regex::Regex;
use std::sync::OnceLock;

fn tag_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Best-effort heuristic to strip "tag blocks".
        // Rust's `regex` crate does not support backreferences, so we can't require matching
        // opening/closing tag names. This is still useful for removing hallucinated XML-like
        // blobs sometimes produced by STT/LLMs.
        Regex::new(r"(?s)<[^>]+>.*?</[^>]+>").expect("valid tag block regex")
    })
}

fn hallucination_brackets_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Remove bracketed hallucinations.
        // Use negated char classes so we don't accidentally remove across multiple brackets.
        Regex::new(r"(?s)\[[^\]]*\]|\([^\)]*\)|\{[^\}]*\}").expect("valid bracket regex")
    })
}

fn filler_words_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Keep list intentionally small for MVP; easy to expand later.
        // Word boundaries are a reasonable heuristic for spaced languages.
        Regex::new(r"(?i)\b(uh|um|uhm|umm|ah|eh|hmm|hm|mmm|mm)\b[,.]?").expect("valid filler regex")
    })
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s{2,}").expect("valid whitespace regex"))
}

fn enhancement_thinking_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)<thinking>.*?</thinking>|<think>.*?</think>|<reasoning>.*?</reasoning>")
            .expect("valid thinking regex")
    })
}

pub fn filter_transcription_output(text: &str) -> String {
    // Mirrors VoiceInk’s intent:
    // - remove <TAG>...</TAG> blocks
    // - remove bracketed hallucinations
    // - remove common filler words
    // - collapse whitespace

    let mut out = text.to_string();

    out = tag_block_re().replace_all(&out, "").to_string();
    out = hallucination_brackets_re()
        .replace_all(&out, "")
        .to_string();
    out = filler_words_re().replace_all(&out, "").to_string();
    out = whitespace_re().replace_all(&out, " ").to_string();

    out.trim().to_string()
}

pub fn filter_enhancement_output(text: &str) -> String {
    // Strip <thinking>, <think>, <reasoning> blocks.
    let out = enhancement_thinking_re().replace_all(text, "");
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_filter_removes_tag_blocks() {
        let input = "hello <TAG>secret</TAG> world";
        assert_eq!(filter_transcription_output(input), "hello world");
    }

    #[test]
    fn transcription_filter_removes_bracketed() {
        let input = "hello [noise] world (uh)";
        assert_eq!(filter_transcription_output(input), "hello world");
    }

    #[test]
    fn enhancement_filter_strips_thinking() {
        let input = "<thinking>plan</thinking>\nResult";
        assert_eq!(filter_enhancement_output(input), "Result");
    }
}

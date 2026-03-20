#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseTextEvent {
    pub delta: Option<String>,
    pub full_text: Option<String>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub done: bool,
}

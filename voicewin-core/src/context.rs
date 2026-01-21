use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextToggles {
    pub use_clipboard: bool,
    pub use_selected_text: bool,
    pub use_window_context: bool,
    pub use_custom_vocabulary: bool,

    // OCR is intentionally deferred; keep flag for forward compatibility.
    pub use_ocr: bool,
}

impl Default for ContextToggles {
    fn default() -> Self {
        Self {
            use_clipboard: true,
            use_selected_text: false,
            use_window_context: true,
            use_custom_vocabulary: true,
            use_ocr: false,
        }
    }
}

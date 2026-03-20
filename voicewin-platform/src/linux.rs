use std::sync::Arc;

use anyhow::Context;
use voicewin_core::llm::visual_capture_scope_label;
use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{
    AppContextProvider, ContextSnapshot, Inserter, ScreenshotCaptureOptions,
};

trait ClipboardBackend: Send + Sync {
    fn get_text(&self) -> anyhow::Result<String>;
    fn set_text(&self, text: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Default)]
struct ArboardClipboard;

impl ClipboardBackend for ArboardClipboard {
    fn get_text(&self) -> anyhow::Result<String> {
        let mut clipboard = arboard::Clipboard::new().context("initialize Linux clipboard")?;
        clipboard.get_text().context("read Linux clipboard text")
    }

    fn set_text(&self, text: &str) -> anyhow::Result<()> {
        let mut clipboard = arboard::Clipboard::new().context("initialize Linux clipboard")?;
        clipboard
            .set_text(text.to_string())
            .context("write Linux clipboard text")
    }
}

#[derive(Clone)]
struct LinuxClipboardHandle(Arc<dyn ClipboardBackend>);

impl Default for LinuxClipboardHandle {
    fn default() -> Self {
        Self(Arc::new(ArboardClipboard))
    }
}

impl std::fmt::Debug for LinuxClipboardHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LinuxClipboardHandle(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxSessionKind {
    Wayland,
    X11,
    Headless,
}

fn current_linux_session_kind() -> LinuxSessionKind {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        LinuxSessionKind::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        LinuxSessionKind::X11
    } else {
        LinuxSessionKind::Headless
    }
}

fn linux_session_kind_label(kind: LinuxSessionKind) -> &'static str {
    match kind {
        LinuxSessionKind::Wayland => "wayland",
        LinuxSessionKind::X11 => "x11",
        LinuxSessionKind::Headless => "headless",
    }
}

fn best_effort_linux_clipboard_text(clipboard: &LinuxClipboardHandle) -> Option<String> {
    match clipboard.0.get_text() {
        Ok(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Err(err) => {
            log::debug!("linux clipboard read unavailable: {err}");
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct LinuxContextProvider {
    clipboard: LinuxClipboardHandle,
}

impl Default for LinuxContextProvider {
    fn default() -> Self {
        Self {
            clipboard: LinuxClipboardHandle::default(),
        }
    }
}

impl LinuxContextProvider {
    #[cfg(test)]
    fn from_clipboard_backend(backend: Arc<dyn ClipboardBackend>) -> Self {
        Self {
            clipboard: LinuxClipboardHandle(backend),
        }
    }
}

#[async_trait::async_trait]
impl AppContextProvider for LinuxContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        let session_kind = current_linux_session_kind();
        log::debug!(
            "linux foreground app lookup not implemented: session_kind={}",
            linux_session_kind_label(session_kind)
        );
        Ok(AppIdentity::new())
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        let session_kind = current_linux_session_kind();
        let clipboard = best_effort_linux_clipboard_text(&self.clipboard);
        log::debug!(
            "linux context snapshot: session_kind={} clipboard_present={} selected_text_supported=false window_context_supported=false",
            linux_session_kind_label(session_kind),
            clipboard.is_some()
        );

        Ok(ContextSnapshot {
            clipboard,
            selected_text: None,
            window_context: None,
            custom_vocabulary: None,
            screenshot: None,
            screenshot_metadata: None,
            precomputed_screen_ocr: None,
        })
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<voicewin_engine::traits::CapturedScreenshot>> {
        log::warn!(
            "linux screenshot capture not implemented: requested_scope={} max_edge_px={}; continuing without screenshot context",
            visual_capture_scope_label(options.scope),
            options.max_edge_px
        );
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub struct LinuxInserter {
    clipboard: LinuxClipboardHandle,
}

impl Default for LinuxInserter {
    fn default() -> Self {
        Self {
            clipboard: LinuxClipboardHandle::default(),
        }
    }
}

impl LinuxInserter {
    #[cfg(test)]
    fn from_clipboard_backend(backend: Arc<dyn ClipboardBackend>) -> Self {
        Self {
            clipboard: LinuxClipboardHandle(backend),
        }
    }
}

#[async_trait::async_trait]
impl Inserter for LinuxInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        self.clipboard
            .0
            .set_text(text)
            .with_context(|| format!("copy transcript to Linux clipboard for mode={mode:?}"))?;
        log::warn!(
            "linux inserter copied transcript to clipboard only: mode={:?} chars={}",
            mode,
            text.chars().count()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MemoryClipboard {
        value: Mutex<Option<String>>,
    }

    impl MemoryClipboard {
        fn with_text(text: &str) -> Self {
            Self {
                value: Mutex::new(Some(text.to_string())),
            }
        }

        fn current_text(&self) -> Option<String> {
            self.value.lock().unwrap().clone()
        }
    }

    impl ClipboardBackend for MemoryClipboard {
        fn get_text(&self) -> anyhow::Result<String> {
            Ok(self.value.lock().unwrap().clone().unwrap_or_default())
        }

        fn set_text(&self, text: &str) -> anyhow::Result<()> {
            *self.value.lock().unwrap() = Some(text.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn linux_context_provider_reads_clipboard_text() {
        let clipboard = Arc::new(MemoryClipboard::with_text("Clipboard note"));
        let provider = LinuxContextProvider::from_clipboard_backend(clipboard);

        let snapshot = provider.snapshot_context().await.unwrap();

        assert_eq!(snapshot.clipboard.as_deref(), Some("Clipboard note"));
        assert_eq!(snapshot.selected_text, None);
        assert_eq!(snapshot.window_context, None);
        assert_eq!(snapshot.screenshot, None);
    }

    #[tokio::test]
    async fn linux_context_provider_does_not_fake_foreground_identity() {
        let provider =
            LinuxContextProvider::from_clipboard_backend(Arc::new(MemoryClipboard::default()));

        let app = provider.foreground_app().await.unwrap();

        assert_eq!(app.process_name, None);
        assert_eq!(app.exe_path, None);
        assert_eq!(app.window_title, None);
    }

    #[tokio::test]
    async fn linux_inserter_copies_text_to_clipboard() {
        let clipboard = Arc::new(MemoryClipboard::default());
        let inserter = LinuxInserter::from_clipboard_backend(clipboard.clone());

        inserter
            .insert("VoiceWin", InsertMode::Paste)
            .await
            .unwrap();

        assert_eq!(clipboard.current_text().as_deref(), Some("VoiceWin"));
    }
}

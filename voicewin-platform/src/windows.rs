// Windows-specific implementations.
//
// We keep them behind `cfg(windows)` so the workspace builds cleanly on Linux.

#[path = "windows_foreground.rs"]
mod windows_foreground;

use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{AppContextProvider, ContextSnapshot, Inserter};

use clipboard_win::get_clipboard_string;

#[path = "windows_insert.rs"]
mod windows_insert;

#[derive(Debug, Default)]
pub struct WindowsContextProvider;

#[async_trait::async_trait]
impl AppContextProvider for WindowsContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        windows_foreground::get_foreground_app_identity()
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        // MVP: provide window/app identity and clipboard text (best-effort).
        let app = self.foreground_app().await?;
        let mut ctx = ContextSnapshot::default();

        let proc = app
            .process_name
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_else(|| "unknown".into());
        let title = app
            .window_title
            .as_ref()
            .map(|t| t.0.clone())
            .unwrap_or_default();

        ctx.window_context = Some(format!("Application: {}\nActive Window: {}", proc, title));
        ctx.clipboard = get_clipboard_string().ok();
        Ok(ctx)
    }
}

#[derive(Debug, Default)]
pub struct WindowsInserter;

#[async_trait::async_trait]
impl Inserter for WindowsInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        // MVP (reliable): clipboard swap + Ctrl+V + optional Enter + restore.
        windows_insert::paste_text_via_clipboard(text, mode)
    }
}

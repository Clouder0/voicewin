//! macOS platform implementations.

mod macos_foreground;
mod macos_insert;

use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{AppContextProvider, ContextSnapshot, Inserter};

#[derive(Debug, Default)]
pub struct MacosContextProvider;

#[async_trait::async_trait]
impl AppContextProvider for MacosContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        macos_foreground::get_foreground_app_identity()
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        // MVP: app identity only; clipboard/context can be added later.
        let app = self.foreground_app().await?;

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

        let mut ctx = ContextSnapshot::default();
        ctx.window_context = Some(format!("Application: {}\nActive Window: {}", proc, title));
        Ok(ctx)
    }
}

#[derive(Debug, Default)]
pub struct MacosInserter;

#[async_trait::async_trait]
impl Inserter for MacosInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        macos_insert::paste_text_via_clipboard(text, mode)
    }
}

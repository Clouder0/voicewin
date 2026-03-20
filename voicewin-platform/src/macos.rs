//! macOS platform implementations.

use std::time::Instant;

#[path = "macos_foreground.rs"]
mod macos_foreground;

#[path = "macos_insert.rs"]
mod macos_insert;

use crate::screenshot::encode_screenshot_data_url;
use core_graphics::display::CGDisplay;
use voicewin_core::context::{ImageArtifact, ScreenshotCaptureMetadata, VisualCaptureScope};
use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{
    AppContextProvider, CapturedScreenshot, ContextSnapshot, Inserter, ScreenshotCaptureOptions,
};

#[derive(Debug, Default)]
pub struct MacosContextProvider;

#[async_trait::async_trait]
impl AppContextProvider for MacosContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        macos_foreground::get_foreground_app_identity()
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
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
        ctx.clipboard = macos_insert::read_clipboard_text();
        ctx.selected_text = macos_insert::read_selected_text();
        Ok(ctx)
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        let started = Instant::now();
        let (scope_label, actual_scope, fallback_reason) = match options.scope {
            VisualCaptureScope::Display => ("display", VisualCaptureScope::Display, None),
            VisualCaptureScope::ForegroundWindow => {
                log::debug!(
                    "macOS screenshot capture: requested_scope=foreground_window fallback_scope=display reason=foreground_window_not_implemented"
                );
                (
                    "display_fallback",
                    VisualCaptureScope::Display,
                    Some("foreground_window_not_implemented".to_string()),
                )
            }
        };
        let image = match CGDisplay::main().image() {
            Some(image) => image,
            None => return Ok(None),
        };

        let width = image.width() as u32;
        let height = image.height() as u32;
        let bits_per_component = image.bits_per_component();
        let bits_per_pixel = image.bits_per_pixel();
        let bytes_per_row = image.bytes_per_row();

        anyhow::ensure!(
            width > 0 && height > 0,
            "captured screenshot has zero dimensions"
        );
        anyhow::ensure!(
            bits_per_component == 8 && bits_per_pixel == 32,
            "unsupported macOS screenshot format: bits_per_component={bits_per_component} bits_per_pixel={bits_per_pixel}"
        );

        let row_bytes = (width as usize)
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("macOS screenshot row length overflow"))?;
        anyhow::ensure!(
            bytes_per_row >= row_bytes,
            "invalid macOS screenshot stride: bytes_per_row={bytes_per_row} row_bytes={row_bytes}"
        );

        let raw = image.data();
        let raw_bytes = raw.bytes();
        let expected_len = (height as usize)
            .checked_mul(bytes_per_row)
            .ok_or_else(|| anyhow::anyhow!("macOS screenshot buffer length overflow"))?;
        anyhow::ensure!(
            raw_bytes.len() >= expected_len,
            "macOS screenshot buffer too small: expected at least {expected_len}, got {}",
            raw_bytes.len()
        );

        let mut bgra = Vec::with_capacity(
            (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| anyhow::anyhow!("macOS screenshot capacity overflow"))?,
        );
        for row in 0..height as usize {
            let start = row
                .checked_mul(bytes_per_row)
                .ok_or_else(|| anyhow::anyhow!("macOS screenshot row offset overflow"))?;
            let end = start
                .checked_add(row_bytes)
                .ok_or_else(|| anyhow::anyhow!("macOS screenshot row slice overflow"))?;
            bgra.extend_from_slice(&raw_bytes[start..end]);
        }

        let artifact = encode_screenshot_data_url(
            width,
            height,
            crate::screenshot::bgra_to_rgba(&bgra)?,
            options.max_edge_px,
        )?;
        let elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        log::debug!(
            "captured macOS screenshot: scope={} width={} height={} elapsed_ms={} data_url_bytes={} max_edge_px={}",
            scope_label,
            width,
            height,
            elapsed_ms,
            artifact.data_url.len(),
            options.max_edge_px
        );
        Ok(Some(CapturedScreenshot {
            image: artifact,
            metadata: ScreenshotCaptureMetadata {
                actual_scope: Some(actual_scope),
                capture_elapsed_ms: Some(elapsed_ms),
                fallback_reason,
            },
        }))
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

/// Returns whether the app is trusted for Accessibility (required for synthetic paste events).
pub fn accessibility_trusted() -> bool {
    macos_insert::is_accessibility_trusted()
}

/// Best-effort: triggers the system prompt for Accessibility trust.
pub fn prompt_accessibility_permission() -> bool {
    macos_insert::prompt_accessibility_permission()
}

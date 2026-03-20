use std::time::Duration;

use std::sync::Arc;
use voicewin_core::context::{ImageArtifact, ScreenshotCaptureMetadata};
use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{
    AppContextProvider, CapturedScreenshot, ContextSnapshot, Inserter, ScreenshotCaptureOptions,
};

#[derive(Debug, Clone)]
pub struct TestContextProvider {
    app: AppIdentity,
    snapshot: ContextSnapshot,
    captured_screenshot: Option<ImageArtifact>,
    captured_screenshot_metadata: Option<ScreenshotCaptureMetadata>,
    snapshot_delay: Duration,
    capture_delay: Duration,
}

impl TestContextProvider {
    pub fn new(app: AppIdentity, snapshot: ContextSnapshot) -> Self {
        Self {
            app,
            snapshot,
            captured_screenshot: None,
            captured_screenshot_metadata: None,
            snapshot_delay: Duration::ZERO,
            capture_delay: Duration::ZERO,
        }
    }

    pub fn with_captured_screenshot(mut self, screenshot: Option<ImageArtifact>) -> Self {
        self.captured_screenshot = screenshot;
        self
    }

    pub fn with_captured_screenshot_metadata(
        mut self,
        metadata: Option<ScreenshotCaptureMetadata>,
    ) -> Self {
        self.captured_screenshot_metadata = metadata;
        self
    }

    pub fn with_snapshot_delay(mut self, delay: Duration) -> Self {
        self.snapshot_delay = delay;
        self
    }

    pub fn with_capture_delay(mut self, delay: Duration) -> Self {
        self.capture_delay = delay;
        self
    }

    pub fn boxed(self) -> Arc<dyn AppContextProvider> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl AppContextProvider for TestContextProvider {
    async fn foreground_app(&self) -> anyhow::Result<AppIdentity> {
        Ok(self.app.clone())
    }

    async fn snapshot_context(&self) -> anyhow::Result<ContextSnapshot> {
        if !self.snapshot_delay.is_zero() {
            tokio::time::sleep(self.snapshot_delay).await;
        }
        Ok(self.snapshot.clone())
    }

    async fn capture_screenshot(
        &self,
        options: ScreenshotCaptureOptions,
    ) -> anyhow::Result<Option<CapturedScreenshot>> {
        if !self.capture_delay.is_zero() {
            tokio::time::sleep(self.capture_delay).await;
        }
        Ok(self
            .captured_screenshot
            .clone()
            .map(|image| CapturedScreenshot {
                image,
                metadata: self.captured_screenshot_metadata.clone().unwrap_or(
                    ScreenshotCaptureMetadata {
                        actual_scope: Some(options.scope),
                        capture_elapsed_ms: (!self.capture_delay.is_zero()).then_some(
                            self.capture_delay
                                .as_millis()
                                .try_into()
                                .unwrap_or(u64::MAX),
                        ),
                        fallback_reason: None,
                    },
                ),
            }))
    }
}

#[derive(Debug, Default)]
pub struct StdoutInserter;

#[async_trait::async_trait]
impl Inserter for StdoutInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        println!("[insert:{:?}] {}", mode, text);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemoryInserter {
    pub inserted: std::sync::Mutex<Vec<(String, InsertMode)>>,
}

#[async_trait::async_trait]
impl Inserter for MemoryInserter {
    async fn insert(&self, text: &str, mode: InsertMode) -> anyhow::Result<()> {
        self.inserted.lock().unwrap().push((text.to_string(), mode));
        Ok(())
    }
}

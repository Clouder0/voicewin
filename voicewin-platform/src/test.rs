use std::sync::Arc;
use voicewin_core::types::{AppIdentity, InsertMode};
use voicewin_engine::traits::{AppContextProvider, ContextSnapshot, Inserter};

#[derive(Debug, Clone)]
pub struct TestContextProvider {
    app: AppIdentity,
    snapshot: ContextSnapshot,
}

impl TestContextProvider {
    pub fn new(app: AppIdentity, snapshot: ContextSnapshot) -> Self {
        Self { app, snapshot }
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
        Ok(self.snapshot.clone())
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

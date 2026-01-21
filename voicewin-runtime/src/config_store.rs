use anyhow::Context;
use std::path::{Path, PathBuf};
use voicewin_core::config::AppConfig;

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> anyhow::Result<AppConfig> {
        let bytes = std::fs::read(&self.path)
            .with_context(|| format!("read config: {}", self.path.display()))?;
        let cfg: AppConfig = serde_json::from_slice(&bytes).context("decode config JSON")?;
        Ok(cfg)
    }

    pub fn save(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(cfg).context("encode config JSON")?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create config directory: {}", parent.display()))?;
        }

        // Atomic-ish write: write temp then replace.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).with_context(|| format!("write temp: {}", tmp.display()))?;
        crate::models::replace_file(&tmp, &self.path)
            .with_context(|| format!("replace file: {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voicewin_core::enhancement::{PromptMode, PromptTemplate};
    use voicewin_core::power_mode::GlobalDefaults;
    use voicewin_core::types::{InsertMode, PromptId};

    #[test]
    fn round_trips_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let store = ConfigStore::at_path(path);

        let cfg = AppConfig {
            defaults: GlobalDefaults {
                enable_enhancement: true,
                prompt_id: None,
                insert_mode: InsertMode::Paste,
                stt_provider: "local".into(),
                stt_model: "mock".into(),
                language: "en".into(),
                llm_base_url: "https://example.com/v1".into(),
                llm_model: "gpt-4o-mini".into(),
                history_enabled: true,
                context: voicewin_core::context::ContextToggles::default(),
            },
            profiles: vec![],
            prompts: vec![PromptTemplate {
                id: PromptId::new(),
                title: "Default".into(),
                mode: PromptMode::Enhancer,
                prompt_text: "Fix.".into(),
                trigger_words: vec!["rewrite".into()],
            }],
            llm_api_key_present: false,
        };

        store.save(&cfg).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.defaults.llm_model, "gpt-4o-mini");
        assert_eq!(loaded.prompts.len(), 1);
    }
}

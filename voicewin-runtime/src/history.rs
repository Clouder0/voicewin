use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts_unix_ms: i64,
    pub app_process_name: Option<String>,
    pub app_exe_path: Option<String>,
    pub app_window_title: Option<String>,
    pub text: String,
    pub stage: String,
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    path: PathBuf,
    max_entries: usize,
}

impl HistoryStore {
    pub fn at_path(path: PathBuf) -> Self {
        Self { path, max_entries: 200 }
    }

    pub fn with_max_entries(mut self, max: usize) -> Self {
        self.max_entries = max.max(1);
        self
    }

    pub fn load(&self) -> anyhow::Result<Vec<HistoryEntry>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }

        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read history: {}", self.path.display()))?;
        let entries: Vec<HistoryEntry> = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse history: {}", self.path.display()))?;
        Ok(entries)
    }

    pub fn append(&self, entry: HistoryEntry) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir: {}", parent.display()))?;
        }

        let mut entries = self.load()?;
        entries.push(entry);
        if entries.len() > self.max_entries {
            let start = entries.len() - self.max_entries;
            entries = entries.split_off(start);
        }

        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&entries)?)
            .with_context(|| format!("failed to write history temp: {}", tmp.display()))?;
        crate::models::replace_file(&tmp, &self.path)
            .with_context(|| format!("failed to replace history: {}", self.path.display()))?;
        Ok(())
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .with_context(|| format!("failed to remove history: {}", self.path.display()))?;
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_limits_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::at_path(dir.path().join("history.json")).with_max_entries(2);

        store
            .append(HistoryEntry {
                ts_unix_ms: 1,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "a".into(),
                stage: "done".into(),
            })
            .unwrap();
        store
            .append(HistoryEntry {
                ts_unix_ms: 2,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "b".into(),
                stage: "done".into(),
            })
            .unwrap();
        store
            .append(HistoryEntry {
                ts_unix_ms: 3,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "c".into(),
                stage: "done".into(),
            })
            .unwrap();

        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "b");
        assert_eq!(entries[1].text, "c");
    }
}

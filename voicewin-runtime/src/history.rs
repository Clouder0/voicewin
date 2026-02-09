use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(default)]
    pub id: String,
    pub ts_unix_ms: i64,
    pub app_process_name: Option<String>,
    pub app_exe_path: Option<String>,
    pub app_window_title: Option<String>,

    // The final text the user can recover/copy.
    pub text: String,

    // UI hint (e.g. "done", "error", "transcribing").
    pub stage: String,

    // Optional error message if the session failed.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    path: PathBuf,
    max_entries: usize,
}

impl HistoryStore {
    pub fn at_path(path: PathBuf) -> Self {
        Self {
            path,
            max_entries: 200,
        }
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
        let mut entries: Vec<HistoryEntry> = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse history: {}", self.path.display()))?;

        if normalize_entry_ids(&mut entries) {
            self.write_entries(&entries)?;
        }

        Ok(entries)
    }

    pub fn append(&self, mut entry: HistoryEntry) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir: {}", parent.display()))?;
        }

        let mut entries = self.load()?;
        ensure_entry_id(&mut entry, &entries);
        entries.push(entry);
        if entries.len() > self.max_entries {
            let start = entries.len() - self.max_entries;
            entries = entries.split_off(start);
        }

        self.write_entries(&entries)
    }

    pub fn delete_entry(&self, ts_unix_ms: i64, text: &str) -> anyhow::Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }

        let mut entries = self.load()?;
        let before = entries.len();

        // Remove the newest matching entry (most user-intentful if duplicates exist).
        if let Some(idx) = entries
            .iter()
            .rposition(|e| e.ts_unix_ms == ts_unix_ms && e.text == text)
        {
            entries.remove(idx);
        }

        if entries.len() == before {
            return Ok(false);
        }

        self.write_entries(&entries)?;
        Ok(true)
    }

    pub fn delete_entry_by_id(&self, id: &str) -> anyhow::Result<bool> {
        if id.trim().is_empty() || !self.path.exists() {
            return Ok(false);
        }

        let mut entries = self.load()?;
        let before = entries.len();

        if let Some(idx) = entries.iter().rposition(|e| e.id == id) {
            entries.remove(idx);
        }

        if entries.len() == before {
            return Ok(false);
        }

        self.write_entries(&entries)?;
        Ok(true)
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

    fn write_entries(&self, entries: &[HistoryEntry]) -> anyhow::Result<()> {
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(entries)?)
            .with_context(|| format!("failed to write history temp: {}", tmp.display()))?;
        crate::models::replace_file(&tmp, &self.path)
            .with_context(|| format!("failed to replace history: {}", self.path.display()))?;
        Ok(())
    }
}

fn ensure_entry_id(entry: &mut HistoryEntry, existing: &[HistoryEntry]) {
    if !entry.id.trim().is_empty() {
        return;
    }

    let used: HashSet<&str> = existing
        .iter()
        .filter_map(|e| {
            if e.id.trim().is_empty() {
                None
            } else {
                Some(e.id.as_str())
            }
        })
        .collect();

    let mut nonce: u64 = 0;
    loop {
        let candidate = format!("{}-{}", entry.ts_unix_ms, nonce);
        if !used.contains(candidate.as_str()) {
            entry.id = candidate;
            break;
        }
        nonce = nonce.saturating_add(1);
    }
}

fn normalize_entry_ids(entries: &mut [HistoryEntry]) -> bool {
    let mut changed = false;
    let mut used = HashSet::new();

    for entry in entries.iter_mut() {
        let existing = entry.id.trim().to_string();
        if !existing.is_empty() && !used.contains(existing.as_str()) {
            used.insert(existing);
            continue;
        }

        let mut nonce: u64 = 0;
        loop {
            let candidate = format!("{}-{}", entry.ts_unix_ms, nonce);
            if !used.contains(candidate.as_str()) {
                entry.id = candidate.clone();
                used.insert(candidate);
                changed = true;
                break;
            }
            nonce = nonce.saturating_add(1);
        }
    }

    changed
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
                id: String::new(),
                ts_unix_ms: 1,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "a".into(),
                stage: "done".into(),
                error: None,
            })
            .unwrap();
        store
            .append(HistoryEntry {
                id: String::new(),
                ts_unix_ms: 2,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "b".into(),
                stage: "done".into(),
                error: None,
            })
            .unwrap();
        store
            .append(HistoryEntry {
                id: String::new(),
                ts_unix_ms: 3,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: "c".into(),
                stage: "done".into(),
                error: None,
            })
            .unwrap();

        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "b");
        assert_eq!(entries[1].text, "c");
        assert!(!entries[0].id.is_empty());
        assert!(!entries[1].id.is_empty());
    }

    #[test]
    fn load_backfills_missing_or_duplicate_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        std::fs::write(
            &path,
            r#"[
  {"ts_unix_ms": 10, "text": "a", "stage": "done", "error": null, "id": ""},
  {"ts_unix_ms": 10, "text": "b", "stage": "done", "error": null, "id": "10-0"}
]"#,
        )
        .unwrap();

        let store = HistoryStore::at_path(path.clone());
        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].id.is_empty());
        assert!(!entries[1].id.is_empty());
        assert_ne!(entries[0].id, entries[1].id);

        let persisted: Vec<HistoryEntry> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(!persisted[0].id.is_empty());
        assert!(!persisted[1].id.is_empty());
    }

    #[test]
    fn delete_entry_by_id_removes_error_only_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::at_path(dir.path().join("history.json"));

        store
            .append(HistoryEntry {
                id: String::new(),
                ts_unix_ms: 100,
                app_process_name: None,
                app_exe_path: None,
                app_window_title: None,
                text: String::new(),
                stage: "error".into(),
                error: Some("boom".into()),
            })
            .unwrap();

        let entries = store.load().unwrap();
        assert_eq!(entries.len(), 1);
        let id = entries[0].id.clone();
        assert!(!id.is_empty());

        assert!(store.delete_entry_by_id(&id).unwrap());
        assert!(store.load().unwrap().is_empty());
    }
}

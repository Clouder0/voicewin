use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

/// File used for app-managed API secret persistence.
const SECRETS_FILENAME: &str = "secrets.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    OpenAiCompatibleApiKey,
    ElevenLabsApiKey,
}

impl SecretKey {
    fn user(self) -> &'static str {
        match self {
            SecretKey::OpenAiCompatibleApiKey => "openai_compatible_api_key",
            SecretKey::ElevenLabsApiKey => "elevenlabs_api_key",
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SecretsFile {
    #[serde(default)]
    entries: BTreeMap<String, String>,
}

static SECRETS_PATH: OnceLock<RwLock<PathBuf>> = OnceLock::new();
static STORE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn secrets_path_cell() -> &'static RwLock<PathBuf> {
    SECRETS_PATH.get_or_init(|| RwLock::new(default_secret_store_path()))
}

fn store_io_lock() -> &'static Mutex<()> {
    STORE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn default_secret_store_path() -> PathBuf {
    std::env::temp_dir().join("voicewin").join(SECRETS_FILENAME)
}

/// Configures where secrets are persisted.
///
/// The path should point to a file (for example: `<app-data-dir>/secrets.json`).
pub fn configure_secret_store_path(path: impl Into<PathBuf>) {
    let mut guard = secrets_path_cell()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = path.into();
}

fn secret_store_path() -> PathBuf {
    secrets_path_cell()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub fn set_secret(key: SecretKey, value: &str) -> anyhow::Result<()> {
    let _guard = store_io_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = secret_store_path();
    let mut store = read_store_file(&path)?;
    store
        .entries
        .insert(key.user().to_string(), value.to_string());
    write_store_file(&path, &store)
}

pub fn get_secret(key: SecretKey) -> anyhow::Result<Option<String>> {
    let _guard = store_io_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = secret_store_path();
    let store = read_store_file(&path)?;
    Ok(store.entries.get(key.user()).cloned())
}

pub fn delete_secret(key: SecretKey) -> anyhow::Result<()> {
    let _guard = store_io_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = secret_store_path();
    let mut store = read_store_file(&path)?;
    if store.entries.remove(key.user()).is_some() {
        write_store_file(&path, &store)?;
    }
    Ok(())
}

fn read_store_file(path: &Path) -> anyhow::Result<SecretsFile> {
    if !path.exists() {
        return Ok(SecretsFile::default());
    }

    let bytes =
        std::fs::read(path).with_context(|| format!("read secret store: {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(SecretsFile::default());
    }

    serde_json::from_slice(&bytes)
        .with_context(|| format!("decode secret store JSON: {}", path.display()))
}

fn write_store_file(path: &Path, store: &SecretsFile) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(store).context("encode secret store JSON")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create secret store directory: {}", parent.display()))?;
    }

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)
        .with_context(|| format!("write secret temp file: {}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set secret file permissions: {}", tmp.display()))?;
    }

    crate::models::replace_file(&tmp, path)
        .with_context(|| format!("replace secret store: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn get_missing_returns_none() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        configure_secret_store_path(dir.path().join("secrets.json"));

        delete_secret(SecretKey::ElevenLabsApiKey).unwrap();
        delete_secret(SecretKey::OpenAiCompatibleApiKey).unwrap();

        assert_eq!(get_secret(SecretKey::ElevenLabsApiKey).unwrap(), None);
        assert_eq!(SecretKey::ElevenLabsApiKey.user(), "elevenlabs_api_key");
    }

    #[test]
    fn set_then_get_round_trips_across_calls() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        configure_secret_store_path(dir.path().join("secrets.json"));

        set_secret(SecretKey::ElevenLabsApiKey, "xi_test_123").unwrap();
        assert_eq!(
            get_secret(SecretKey::ElevenLabsApiKey).unwrap(),
            Some("xi_test_123".to_string())
        );
    }

    #[test]
    fn delete_secret_only_removes_selected_key() {
        let _test_guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempdir().unwrap();
        configure_secret_store_path(dir.path().join("secrets.json"));

        set_secret(SecretKey::ElevenLabsApiKey, "xi_test_123").unwrap();
        set_secret(SecretKey::OpenAiCompatibleApiKey, "sk_test_123").unwrap();

        delete_secret(SecretKey::ElevenLabsApiKey).unwrap();

        assert_eq!(get_secret(SecretKey::ElevenLabsApiKey).unwrap(), None);
        assert_eq!(
            get_secret(SecretKey::OpenAiCompatibleApiKey).unwrap(),
            Some("sk_test_123".to_string())
        );
    }
}

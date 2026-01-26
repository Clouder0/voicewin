use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::Digest;

pub const BOOTSTRAP_MODEL_FILENAME: &str = "bootstrap.gguf";

// Bundled bootstrap model:
// https://huggingface.co/FL33TW00D-HF/whisper-base (base_q4k.gguf)
// We keep the filename stable in-app.
pub const BOOTSTRAP_MODEL_SHA256: &str =
    "002978331fb3fb35c9939ff2ca227bf2d6658fc666fe56415fb5eac0a839b60f";

// ~42MB for base_q4k.gguf. Use a conservative lower bound to catch empty/corrupt bundles.
pub const BOOTSTRAP_MODEL_MIN_BYTES: u64 = 10 * 1024 * 1024;

pub const PREFERRED_LOCAL_STT_MODEL_FILENAME: &str = "whisper-large-v3-turbo-q5_k.gguf";

pub fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create dir: {}", path.display()))
}

pub fn atomic_copy(src: &Path, dst: &Path) -> anyhow::Result<()> {
    // Streaming copy so large GGUF files don't spike RAM.
    // Also handles replacement safely on Windows, where `rename` fails if the destination exists.
    let tmp = dst.with_extension("tmp");
    if let Some(parent) = dst.parent() {
        ensure_dir(parent)?;
    }

    let mut r =
        fs::File::open(src).with_context(|| format!("failed to open: {}", src.display()))?;
    let mut w =
        fs::File::create(&tmp).with_context(|| format!("failed to create: {}", tmp.display()))?;

    if let Err(e) = std::io::copy(&mut r, &mut w) {
        // Best-effort cleanup so we don't leave partial temp files around.
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::Error::new(e).context(format!(
            "failed copying {} -> {}",
            src.display(),
            tmp.display()
        )));
    }

    w.sync_all().ok();

    replace_file(&tmp, dst)
}

pub fn file_size_bytes(path: &Path) -> anyhow::Result<u64> {
    let m = fs::metadata(path).with_context(|| format!("failed to stat: {}", path.display()))?;
    Ok(m.len())
}

pub fn has_gguf_magic(path: &Path) -> anyhow::Result<bool> {
    let mut f =
        fs::File::open(path).with_context(|| format!("failed to open: {}", path.display()))?;
    let mut magic = [0u8; 4];
    let n = f
        .read(&mut magic)
        .with_context(|| format!("failed reading: {}", path.display()))?;

    Ok(n == 4 && magic == *b"GGUF")
}

pub fn validate_gguf_file(path: &Path, min_bytes: u64) -> anyhow::Result<()> {
    let len = file_size_bytes(path)?;
    if len < min_bytes {
        return Err(anyhow::anyhow!(
            "model file too small ({} bytes, expected >= {}): {}",
            len,
            min_bytes,
            path.display()
        ));
    }

    if !has_gguf_magic(path)? {
        return Err(anyhow::anyhow!(
            "model file is not GGUF (missing magic header): {}",
            path.display()
        ));
    }

    Ok(())
}

pub fn replace_file(tmp: &Path, dst: &Path) -> anyhow::Result<()> {
    let backup = dst.with_extension("bak");

    if dst.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(dst, &backup)
            .with_context(|| format!("failed rename {} -> {}", dst.display(), backup.display()))?;
    }

    if let Err(e) = fs::rename(tmp, dst) {
        // Try to restore previous file if we had one.
        if backup.exists() {
            let _ = fs::rename(&backup, dst);
        }
        let _ = fs::remove_file(tmp);
        return Err(anyhow::Error::new(e).context(format!(
            "failed rename {} -> {}",
            tmp.display(),
            dst.display()
        )));
    }

    let _ = fs::remove_file(&backup);
    Ok(())
}

pub fn validate_bootstrap_model(path: &Path) -> anyhow::Result<()> {
    validate_gguf_file(path, BOOTSTRAP_MODEL_MIN_BYTES)?;

    let hash = sha256_file(path)?;
    if hash != BOOTSTRAP_MODEL_SHA256 {
        return Err(anyhow::anyhow!(
            "bootstrap model checksum mismatch (expected {}, got {}): {}",
            BOOTSTRAP_MODEL_SHA256,
            hash,
            path.display()
        ));
    }

    Ok(())
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f =
        fs::File::open(path).with_context(|| format!("failed to open: {}", path.display()))?;
    let mut hasher = sha2::Sha256::new();

    let mut buf = [0u8; 8192];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("failed reading: {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn models_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("models")
}

pub fn installed_bootstrap_model_path(app_data_dir: &Path) -> PathBuf {
    models_dir(app_data_dir).join(BOOTSTRAP_MODEL_FILENAME)
}

pub fn installed_preferred_local_stt_model_path(app_data_dir: &Path) -> PathBuf {
    models_dir(app_data_dir).join(PREFERRED_LOCAL_STT_MODEL_FILENAME)
}

pub fn choose_default_local_stt_model_path(app_data_dir: &Path) -> PathBuf {
    let preferred = installed_preferred_local_stt_model_path(app_data_dir);
    if preferred.exists() {
        preferred
    } else {
        installed_bootstrap_model_path(app_data_dir)
    }
}

#[derive(Debug, Clone)]
pub struct ModelDownloadSpec {
    pub id: String,
    pub title: String,

    pub url: String,

    // Pinned integrity: many public GGUF repos do not publish sidecar .sha256 files.
    pub sha256: String,

    pub filename: String,

    pub size_bytes: Option<u64>,
    pub speed_label: Option<String>,
    pub accuracy_label: Option<String>,
    pub recommended: bool,
}

pub fn whisper_catalog() -> Vec<ModelDownloadSpec> {
    // Minimal GGUF catalog with pinned checksums.
    // Sources: FL33TW00D-HF Whisper GGUF repos.
    vec![
        ModelDownloadSpec {
            id: "whisper-base-q4k".into(),
            title: "Whisper Base".into(),
            url: "https://huggingface.co/FL33TW00D-HF/whisper-base/resolve/main/base_q4k.gguf".into(),
            sha256: "002978331fb3fb35c9939ff2ca227bf2d6658fc666fe56415fb5eac0a839b60f".into(),
            filename: "base_q4k.gguf".into(),
            size_bytes: Some(44_295_968),
            speed_label: Some("Fast".into()),
            accuracy_label: Some("Good".into()),
            recommended: true,
        },
        ModelDownloadSpec {
            id: "whisper-medium-q4k".into(),
            title: "Whisper Medium".into(),
            url: "https://huggingface.co/FL33TW00D-HF/whisper-medium/resolve/main/medium_q4k.gguf".into(),
            sha256: "88f744da1c6cc2273d226839bd28dd98aab988ef915e47b325b82340f135fb34".into(),
            filename: "medium_q4k.gguf".into(),
            size_bytes: Some(443_874_912),
            speed_label: Some("Slow".into()),
            accuracy_label: Some("High".into()),
            recommended: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");
        fs::write(&path, b"").unwrap();
        let hash = sha256_file(&path).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::Digest;

pub const BOOTSTRAP_MODEL_FILENAME: &str = "bootstrap.bin";

// Bundled bootstrap model (whisper.cpp GGML format):
// Source: https://huggingface.co/ggerganov/whisper.cpp (ggml-tiny.bin)
// We keep the filename stable in-app.
pub const BOOTSTRAP_MODEL_SHA256: &str =
    "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21";

// ~75MB for ggml-tiny.bin. Use a conservative lower bound to catch empty/corrupt bundles.
pub const BOOTSTRAP_MODEL_MIN_BYTES: u64 = 10 * 1024 * 1024;

// "Preferred" local model filename checked during config initialization.
// If present, we pick it over the bundled bootstrap.
pub const PREFERRED_LOCAL_STT_MODEL_FILENAME: &str = "ggml-base-q5_1.bin";

pub fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create dir: {}", path.display()))
}

pub fn atomic_copy(src: &Path, dst: &Path) -> anyhow::Result<()> {
    // Streaming copy so large model files don't spike RAM.
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

pub fn has_ggml_magic(path: &Path) -> anyhow::Result<bool> {
    let mut f =
        fs::File::open(path).with_context(|| format!("failed to open: {}", path.display()))?;
    let mut magic = [0u8; 4];
    let n = f
        .read(&mut magic)
        .with_context(|| format!("failed reading: {}", path.display()))?;

    // whisper.cpp's GGML format uses a little-endian u32 magic which appears as "lmgg" bytes.
    Ok(n == 4 && magic == *b"lmgg")
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

pub fn validate_ggml_file(path: &Path, min_bytes: u64) -> anyhow::Result<()> {
    let len = file_size_bytes(path)?;
    if len < min_bytes {
        return Err(anyhow::anyhow!(
            "model file too small ({} bytes, expected >= {}): {}",
            len,
            min_bytes,
            path.display()
        ));
    }

    if !has_ggml_magic(path)? {
        return Err(anyhow::anyhow!(
            "model file is not whisper.cpp GGML (.bin) (missing magic header): {}",
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
    validate_ggml_file(path, BOOTSTRAP_MODEL_MIN_BYTES)?;

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
    pub alt_url: Option<String>,

    // Pinned integrity: many public GGUF repos do not publish sidecar .sha256 files.
    pub sha256: String,

    pub filename: String,

    pub size_bytes: Option<u64>,
    pub speed_label: Option<String>,
    pub accuracy_label: Option<String>,
    pub recommended: bool,
}

pub fn whisper_catalog() -> Vec<ModelDownloadSpec> {
    // Minimal GGML (.bin) catalog with pinned checksums.
    // Source: ggerganov/whisper.cpp models.
    //
    // NOTE: `hf-mirror.com` is used as the primary URL because it works in more restrictive
    // network environments; we keep the official Hugging Face URL as a fallback.
    const WHISPER_CPP_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

    vec![
        ModelDownloadSpec {
            id: "whisper-base-q5_1".into(),
            title: "Whisper Base".into(),
            url: format!(
                "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/{WHISPER_CPP_COMMIT}/ggml-base-q5_1.bin"
            ),
            alt_url: Some(format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/{WHISPER_CPP_COMMIT}/ggml-base-q5_1.bin"
            )),
            sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898".into(),
            filename: "ggml-base-q5_1.bin".into(),
            size_bytes: Some(59_707_625),
            speed_label: Some("Medium".into()),
            accuracy_label: Some("High".into()),
            recommended: true,
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

    #[test]
    fn detects_ggml_magic_lmgg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.bin");

        // Minimal header for our validator: 4-byte magic + padding.
        fs::write(&path, [b"lmgg".as_slice(), &[0u8; 8]].concat()).unwrap();

        assert!(has_ggml_magic(&path).unwrap());
        validate_ggml_file(&path, 4).unwrap();
    }

    #[test]
    fn rejects_gguf_when_ggml_expected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        fs::write(&path, [b"GGUF".as_slice(), &[0u8; 8]].concat()).unwrap();

        assert!(has_gguf_magic(&path).unwrap());
        let err = validate_ggml_file(&path, 4).unwrap_err();
        assert!(err
            .to_string()
            .contains("not whisper.cpp GGML"));
    }
}

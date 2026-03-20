use std::path::Path;

use anyhow::{Context, anyhow};
use base64::Engine;
use voicewin_core::context::ImageArtifact;

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub artifact: ImageArtifact,
    pub source: String,
    pub mime_type: String,
    pub bytes: usize,
}

pub fn load_optional_image_from_env(
    data_url_vars: &[&str],
    path_vars: &[&str],
) -> anyhow::Result<Option<LoadedImage>> {
    for name in data_url_vars {
        let Some(raw) = std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let parsed = voicewin_providers::data_url::parse_base64_image_data_url(&raw)
            .with_context(|| format!("parse image data URL from {name}"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&parsed.base64_data)
            .with_context(|| format!("decode image data URL bytes from {name}"))?
            .len();
        return Ok(Some(LoadedImage {
            artifact: ImageArtifact { data_url: raw },
            source: format!("env:{name}"),
            mime_type: parsed.mime_type,
            bytes,
        }));
    }

    for name in path_vars {
        let Some(raw_path) = std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let path = Path::new(&raw_path);
        let bytes = std::fs::read(path)
            .with_context(|| format!("read image file from {name}: {}", path.display()))?;
        let mime_type = detect_image_mime_type(path, &bytes)?;
        let data_url = format!(
            "data:{mime_type};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );

        return Ok(Some(LoadedImage {
            artifact: ImageArtifact { data_url },
            source: format!("path:{}", path.display()),
            mime_type,
            bytes: bytes.len(),
        }));
    }

    Ok(None)
}

fn detect_image_mime_type(path: &Path, bytes: &[u8]) -> anyhow::Result<String> {
    if let Some(ext) = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
    {
        match ext.as_str() {
            "png" => return Ok("image/png".into()),
            "jpg" | "jpeg" => return Ok("image/jpeg".into()),
            "webp" => return Ok("image/webp".into()),
            "gif" => return Ok("image/gif".into()),
            _ => {}
        }
    }

    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Ok("image/png".into());
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok("image/jpeg".into());
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok("image/gif".into());
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp".into());
    }

    Err(anyhow!(
        "unsupported image format for {}; use png, jpeg, webp, or gif",
        path.display()
    ))
}

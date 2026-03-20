use anyhow::Context;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDataUrl {
    pub mime_type: String,
    pub base64_data: String,
}

pub fn parse_base64_image_data_url(value: &str) -> anyhow::Result<ParsedDataUrl> {
    let trimmed = value.trim();
    let rest = trimmed
        .strip_prefix("data:")
        .ok_or_else(|| anyhow::anyhow!("image must be a data URL"))?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("image data URL is missing payload"))?;

    let mut parts = meta.split(';');
    let mime_type = parts
        .next()
        .map(str::trim)
        .filter(|mime| !mime.is_empty())
        .ok_or_else(|| anyhow::anyhow!("image data URL is missing MIME type"))?;

    if !mime_type.starts_with("image/") {
        anyhow::bail!("image data URL must use an image/* MIME type");
    }

    let is_base64 = parts.any(|part| part.trim().eq_ignore_ascii_case("base64"));
    if !is_base64 {
        anyhow::bail!("image data URL must be base64-encoded");
    }

    let base64_data = payload.trim();
    if base64_data.is_empty() {
        anyhow::bail!("image data URL payload is empty");
    }

    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_data)
        .with_context(|| "image data URL contains invalid base64")?;

    Ok(ParsedDataUrl {
        mime_type: mime_type.to_string(),
        base64_data: base64_data.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_png_data_url() {
        let parsed = parse_base64_image_data_url("data:image/png;base64,SGVsbG8=").unwrap();
        assert_eq!(parsed.mime_type, "image/png");
        assert_eq!(parsed.base64_data, "SGVsbG8=");
    }

    #[test]
    fn rejects_non_image_data_url() {
        let err = parse_base64_image_data_url("data:text/plain;base64,SGVsbG8=").unwrap_err();
        assert!(err.to_string().contains("image/* MIME type"));
    }
}

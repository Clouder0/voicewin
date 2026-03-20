use anyhow::Context;
use base64::Engine;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::FilterType as ResizeFilterType;
use image::{ColorType, DynamicImage, ImageEncoder, RgbaImage};
use voicewin_core::context::ImageArtifact;

pub const DEFAULT_SCREENSHOT_MAX_EDGE_PX: u32 = 1280;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedScreenshot {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn normalized_max_edge_px(value: u32) -> u32 {
    value.clamp(256, 3_840)
}

fn resized_dimensions(width: u32, height: u32, max_edge_px: u32) -> (u32, u32) {
    let max_edge_px = normalized_max_edge_px(max_edge_px);
    let longest_edge = width.max(height);
    if longest_edge <= max_edge_px {
        return (width, height);
    }

    let scale = max_edge_px as f64 / longest_edge as f64;
    let scaled_width = ((width as f64) * scale).round() as u32;
    let scaled_height = ((height as f64) * scale).round() as u32;
    (scaled_width.max(1), scaled_height.max(1))
}

fn prepare_rgba_screenshot(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    max_edge_px: u32,
) -> anyhow::Result<PreparedScreenshot> {
    let expected_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .context("screenshot dimensions overflow")?;
    anyhow::ensure!(
        rgba.len() == expected_len,
        "unexpected screenshot buffer length: expected {expected_len}, got {}",
        rgba.len()
    );

    let image =
        RgbaImage::from_raw(width, height, rgba).context("construct RGBA screenshot image")?;
    let (target_width, target_height) = resized_dimensions(width, height, max_edge_px);
    if target_width == width && target_height == height {
        return Ok(PreparedScreenshot {
            width,
            height,
            rgba: image.into_raw(),
        });
    }

    let resized = DynamicImage::ImageRgba8(image)
        .resize(target_width, target_height, ResizeFilterType::Triangle)
        .to_rgba8();
    Ok(PreparedScreenshot {
        width: target_width,
        height: target_height,
        rgba: resized.into_raw(),
    })
}

pub(crate) fn bgra_to_rgba(buffer: &[u8]) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        buffer.len() % 4 == 0,
        "BGRA buffer length must be divisible by four"
    );

    let mut rgba = Vec::with_capacity(buffer.len());
    for pixel in buffer.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    Ok(rgba)
}

pub(crate) fn encode_screenshot_data_url(
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    max_edge_px: u32,
) -> anyhow::Result<ImageArtifact> {
    let prepared = prepare_rgba_screenshot(width, height, rgba, max_edge_px)?;
    let mut png = Vec::new();
    let encoder =
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter);
    encoder
        .write_image(
            &prepared.rgba,
            prepared.width,
            prepared.height,
            ColorType::Rgba8.into(),
        )
        .context("encode screenshot PNG")?;

    Ok(ImageArtifact {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SCREENSHOT_MAX_EDGE_PX, bgra_to_rgba, encode_screenshot_data_url,
        normalized_max_edge_px, prepare_rgba_screenshot, resized_dimensions,
    };

    #[test]
    fn resized_dimensions_leave_small_images_unchanged() {
        assert_eq!(
            resized_dimensions(960, 540, DEFAULT_SCREENSHOT_MAX_EDGE_PX),
            (960, 540)
        );
    }

    #[test]
    fn resized_dimensions_clamp_longest_edge() {
        let (width, height) = resized_dimensions(3840, 2160, DEFAULT_SCREENSHOT_MAX_EDGE_PX);
        assert_eq!(width, DEFAULT_SCREENSHOT_MAX_EDGE_PX);
        assert!(height < DEFAULT_SCREENSHOT_MAX_EDGE_PX);
    }

    #[test]
    fn normalized_max_edge_px_clamps_to_supported_range() {
        assert_eq!(normalized_max_edge_px(0), 256);
        assert_eq!(normalized_max_edge_px(1280), 1280);
        assert_eq!(normalized_max_edge_px(4000), 3840);
    }

    #[test]
    fn prepare_rgba_screenshot_resizes_large_frames() {
        let source = vec![255u8; 3840 * 2160 * 4];
        let prepared =
            prepare_rgba_screenshot(3840, 2160, source, DEFAULT_SCREENSHOT_MAX_EDGE_PX).unwrap();
        assert_eq!(prepared.width, DEFAULT_SCREENSHOT_MAX_EDGE_PX);
        assert!(prepared.height < DEFAULT_SCREENSHOT_MAX_EDGE_PX);
        assert_eq!(
            prepared.rgba.len(),
            prepared.width as usize * prepared.height as usize * 4
        );
    }

    #[test]
    fn prepare_rgba_screenshot_honors_smaller_requested_max_edge() {
        let source = vec![255u8; 3840 * 2160 * 4];
        let prepared = prepare_rgba_screenshot(3840, 2160, source, 640).unwrap();
        assert_eq!(prepared.width, 640);
        assert!(prepared.height < 640);
    }

    #[test]
    fn bgra_to_rgba_swaps_channels() {
        let rgba = bgra_to_rgba(&[1, 2, 3, 4, 10, 11, 12, 13]).unwrap();
        assert_eq!(rgba, vec![3, 2, 1, 4, 12, 11, 10, 13]);
    }

    #[test]
    fn encode_screenshot_data_url_emits_png_data_url() {
        let artifact =
            encode_screenshot_data_url(1, 1, vec![255, 0, 0, 255], DEFAULT_SCREENSHOT_MAX_EDGE_PX)
                .unwrap();
        assert!(artifact.data_url.starts_with("data:image/png;base64,"));
    }
}

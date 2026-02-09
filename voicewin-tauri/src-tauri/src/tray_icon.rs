pub fn load_embedded_tray_icon() -> Option<tauri::image::Image<'static>> {
    const ICON_BYTES: &[u8] = include_bytes!("../icons/32x32.png");

    match tauri::image::Image::from_bytes(ICON_BYTES) {
        Ok(icon) => Some(icon.to_owned()),
        Err(e) => {
            log::error!("failed to decode embedded tray icon: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_embedded_tray_icon;

    #[test]
    fn embedded_tray_icon_decodes() {
        let icon = load_embedded_tray_icon();
        assert!(icon.is_some());
    }
}

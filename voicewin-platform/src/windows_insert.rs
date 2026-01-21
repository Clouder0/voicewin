// Windows clipboard-based paste.
//
// Goal: reliably paste into the currently focused app.
// Strategy:
// - Save current clipboard text (best-effort)
// - Write our text to clipboard
// - Send Ctrl+V (then optional Enter)
// - Restore clipboard

#![cfg(windows)]

use std::thread;
use std::time::Duration;

use clipboard_win::{get_clipboard_string, set_clipboard_string};
use enigo::Keyboard;
use voicewin_core::types::InsertMode;

fn send_paste_ctrl_v(enigo: &mut enigo::Enigo) -> anyhow::Result<()> {
    // VK_V (0x56) avoids layout issues.
    enigo
        .key(enigo::Key::Control, enigo::Direction::Press)
        .map_err(|e| anyhow::anyhow!("failed to press Ctrl: {e}"))?;
    enigo
        .key(enigo::Key::Other(0x56), enigo::Direction::Click)
        .map_err(|e| anyhow::anyhow!("failed to press V: {e}"))?;
    thread::sleep(Duration::from_millis(100));
    enigo
        .key(enigo::Key::Control, enigo::Direction::Release)
        .map_err(|e| anyhow::anyhow!("failed to release Ctrl: {e}"))?;
    Ok(())
}

pub fn paste_text_via_clipboard(text: &str, mode: InsertMode) -> anyhow::Result<()> {
    let original = get_clipboard_string().ok();

    set_clipboard_string(text).map_err(|e| anyhow::anyhow!("failed to write clipboard: {e}"))?;
    thread::sleep(Duration::from_millis(50));

    let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
        .map_err(|e| anyhow::anyhow!("failed to init enigo: {e}"))?;

    send_paste_ctrl_v(&mut enigo)?;

    if matches!(mode, InsertMode::PasteAndEnter) {
        thread::sleep(Duration::from_millis(50));
        enigo
            .key(enigo::Key::Return, enigo::Direction::Click)
            .map_err(|e| anyhow::anyhow!("failed to press enter: {e}"))?;
    }

    thread::sleep(Duration::from_millis(50));
    if let Some(original) = original {
        let _ = set_clipboard_string(&original);
    }

    Ok(())
}

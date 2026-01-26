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

fn send_paste_shift_insert(enigo: &mut enigo::Enigo) -> anyhow::Result<()> {
    // VK_INSERT = 0x2D
    enigo
        .key(enigo::Key::Shift, enigo::Direction::Press)
        .map_err(|e| anyhow::anyhow!("failed to press Shift: {e}"))?;
    enigo
        .key(enigo::Key::Other(0x2D), enigo::Direction::Click)
        .map_err(|e| anyhow::anyhow!("failed to press Insert: {e}"))?;
    thread::sleep(Duration::from_millis(100));
    enigo
        .key(enigo::Key::Shift, enigo::Direction::Release)
        .map_err(|e| anyhow::anyhow!("failed to release Shift: {e}"))?;
    Ok(())
}

fn send_paste(enigo: &mut enigo::Enigo, mode: InsertMode) -> anyhow::Result<()> {
    match mode {
        InsertMode::ShiftInsert => send_paste_shift_insert(enigo),
        _ => send_paste_ctrl_v(enigo),
    }
}

pub fn paste_text_via_clipboard(text: &str, mode: InsertMode) -> anyhow::Result<()> {
    // Preserve user's clipboard and always attempt to restore it.
    // If the paste/enter keystrokes fail, the error propagates, but restoration
    // should still happen.

    let original = get_clipboard_string().ok();

    // 1) Put our text on clipboard.
    set_clipboard_string(text).map_err(|e| anyhow::anyhow!("failed to write clipboard: {e}"))?;
    thread::sleep(Duration::from_millis(50));

    // 2) Send paste keystroke.
    let paste_result = (|| {
        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| anyhow::anyhow!("failed to init enigo: {e}"))?;

        send_paste(&mut enigo, mode)?;

        if matches!(mode, InsertMode::PasteAndEnter) {
            thread::sleep(Duration::from_millis(50));
            enigo
                .key(enigo::Key::Return, enigo::Direction::Click)
                .map_err(|e| anyhow::anyhow!("failed to press enter: {e}"))?;
        }

        Ok::<(), anyhow::Error>(())
    })();

    // 3) Always restore user's clipboard (best-effort).
    thread::sleep(Duration::from_millis(50));
    if let Some(original) = original {
        let _ = set_clipboard_string(&original);
    }

    // 4) Return the keystroke result.
    paste_result
}

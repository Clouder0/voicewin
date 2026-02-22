// macOS foreground app detection.
//
// Implementation notes:
// - For MVP we return best-effort information from NSWorkspace.frontmostApplication.
// - Window title is intentionally omitted here (requires Accessibility for most approaches).

#![cfg(target_os = "macos")]

use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};

use voicewin_core::types::AppIdentity;

pub fn get_foreground_app_identity() -> anyhow::Result<AppIdentity> {
    // Accessing AppKit APIs is generally expected on the main thread, but in
    // practice reading `frontmostApplication` is commonly done off-main.
    let app: Option<Retained<NSRunningApplication>> =
        NSWorkspace::sharedWorkspace().frontmostApplication();

    let mut out = AppIdentity::new();
    let Some(app) = app else {
        return Ok(out);
    };

    // Prefer bundle identifier as an exe_path-like stable identifier.
    if let Some(bundle_id) = app.bundleIdentifier() {
        out = out.with_exe_path(bundle_id.to_string());
    }

    if let Some(name) = app.localizedName() {
        out = out.with_process_name(name.to_string());
    }

    Ok(out)
}

// macOS foreground app detection.
//
// MVP: best-effort stub. Real implementation will use NSWorkspace/Accessibility APIs.

use voicewin_core::types::AppIdentity;

pub fn get_foreground_app_identity() -> anyhow::Result<AppIdentity> {
    // TODO(macos): implement foreground app detection.
    Ok(AppIdentity::new())
}

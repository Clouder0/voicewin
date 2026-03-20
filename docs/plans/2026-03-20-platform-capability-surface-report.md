# Platform Capability Surface Report

Date: 2026-03-20

## Scope

Made platform support explicit for profile matching and context capture so Settings and Profiles can warn users about real OS constraints instead of implying best-effort behavior that does not exist yet.

This pass also stabilized the frontend tests so they declare the platform capability contract they depend on.

## Changes

### 1. Added a backend platform capability command

Added `get_platform_capabilities` in `voicewin-tauri/src-tauri/src/main.rs`.

The command returns a serialized `PlatformCapabilities` shape with:

- `platform`
- `foreground_app_identity`
- `clipboard_context`
- `selected_text_context`
- `window_context`
- `screenshot_capture`
- `foreground_window_capture`
- `auto_insert`

Current behavior is intentionally honest:

- Windows reports full support for the current capability surface
- macOS reports screenshot support but no foreground-window screenshot capture yet
- Linux reports clipboard-only support, with no foreground-app identity, selected-text capture, window context, screenshot capture, or auto-insert

### 2. Added shared frontend capability interpretation

Added `voicewin-tauri/src/ui/platformCapabilities.ts` and tests for it.

The helper provides:

- navigator-based fallback capability inference when the Tauri command is unavailable
- human-readable platform labels
- context warning generation for selected text, window context, and visual capture
- foreground-app warning generation for profile matching

This keeps product copy consistent between Settings and Profiles.

### 3. Wired capability warnings into Settings and Profiles

Updated:

- `voicewin-tauri/src/ui/SettingsPage.tsx`
- `voicewin-tauri/src/ui/ProfilesPage.tsx`

Behavior:

- both pages attempt `get_platform_capabilities` during refresh
- both fall back to inferred capabilities if the command is unavailable
- unsupported Linux paths now show explicit warnings instead of optimistic help text
- the profile matcher UI disables `Use Foreground` when foreground app identity is unavailable
- screenshot/OCR guidance is suppressed when the platform cannot actually provide visual capture

### 4. Removed fake Linux foreground-app identity

Updated `voicewin-platform/src/linux.rs` so `foreground_app()` returns an empty `AppIdentity` instead of faking `"linux"` as a process name.

Added regression coverage to ensure Linux does not pretend profile matching works when it does not.

This matters because profile routing should degrade explicitly, not silently match on synthetic data.

### 5. Stabilized test assumptions

Frontend tests that depend on screenshot support or foreground-app capture now mock an explicit Windows capability surface instead of relying on JSDOM user-agent fallback behavior.

Updated:

- `voicewin-tauri/src/ui/SettingsPage.test.tsx`
- `voicewin-tauri/src/ui/ProfilesPage.test.tsx`

This keeps the tests aligned with the product design:

- tests expecting screenshot info boxes now declare screenshot support
- tests expecting `Use Foreground` to work now declare foreground-app identity support
- Linux-specific warning tests still mock Linux explicitly

## Verification

Passed locally:

- `npm test` in `voicewin-tauri`
- `npm run typecheck` in `voicewin-tauri`
- `cargo test -q -p voicewin-platform`
- `env CARGO_HOME=/tmp/voicewin-cargo-home cargo check -q` in `voicewin-tauri/src-tauri`

## Outcome

VoiceWin now exposes a clearer contract around platform-dependent behavior:

- unsupported capture paths are surfaced before users depend on them
- Linux behavior is explicit rather than synthetic
- the frontend test suite now models platform support deliberately instead of inheriting accidental environment defaults

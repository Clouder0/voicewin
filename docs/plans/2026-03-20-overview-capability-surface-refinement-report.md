# Overview Capability Surface Refinement Report

Date: 2026-03-20

## Scope

Extended the explicit platform-capability contract into the Overview page so the main UI no longer relies on user-agent guesses alone for Linux fallback messaging.

This pass also centralized frontend capability loading so Settings, Profiles, and Overview all resolve platform support through the same helper.

## Changes

### 1. Added a shared capability loader

Updated `voicewin-tauri/src/ui/platformCapabilities.ts` with:

- `loadPlatformCapabilities(...)`

Behavior:

- prefer the backend `get_platform_capabilities` command when available
- fall back to navigator inference only when the backend command is unavailable

This removes repeated inline `try/catch` capability-loading logic from multiple pages.

### 2. Reused the helper in Settings and Profiles

Updated:

- `voicewin-tauri/src/ui/SettingsPage.tsx`
- `voicewin-tauri/src/ui/ProfilesPage.tsx`

This is a small refactor, but it matters because capability loading now has one frontend entry point instead of page-specific copies.

### 3. Wired Overview to the capability surface

Updated `voicewin-tauri/src/ui/OverviewPage.tsx`.

Behavior changes:

- platform state now starts from conservative fallback capabilities and upgrades to backend-reported capabilities when Tauri is available
- Linux fallback messaging is now driven by the explicit platform capability result
- the Linux card copy now states the actual unsupported features instead of calling them “best-effort”
- the third status card now reflects insert behavior:
  - `Auto Insert`
  - `Clipboard Only`
- macOS accessibility status is refreshed on initial page load again, not only on later focus/visibility events

### 4. Added regression coverage

Updated:

- `voicewin-tauri/src/ui/platformCapabilities.test.ts`
- `voicewin-tauri/src/ui/OverviewPage.linux.test.tsx`
- `voicewin-tauri/src/ui/OverviewPage.accessibility.test.tsx`
- `voicewin-tauri/src/ui/OverviewPage.microphone.test.tsx`

New coverage includes:

- backend capability result overriding user-agent fallback
- Linux Overview messaging when the backend explicitly reports Linux constraints
- shared capability loader fallback behavior
- Overview startup still surfacing macOS accessibility CTA correctly

## Verification

Passed locally:

- `npm test` in `voicewin-tauri`
- `npm run typecheck` in `voicewin-tauri`

## Outcome

The capability surface is now more coherent across the frontend:

- Settings, Profiles, and Overview all resolve platform support the same way
- the main page no longer implies Linux support with vague language
- backend capability truth now takes precedence over browser-environment heuristics

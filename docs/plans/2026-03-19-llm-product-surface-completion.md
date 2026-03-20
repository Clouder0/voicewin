# LLM Product Surface Completion

Date: 2026-03-19

## Goal

Finish the missing user-facing surfaces around the LLM post-processing work that is already
implemented in core/runtime, so the feature is actually operable instead of remaining mostly
hidden plumbing.

This batch intentionally does **not** attempt the larger prompt-lab/history redesign. It closes
the highest-value usability gaps that block practical use today.

## Problems Confirmed In The Repo

1. `ProfilesPage` still says the override UI is stubbed even though the engine already supports:
   - per-profile enhancement enable/disable
   - prompt overrides
   - provider/model/base-url overrides
   - API mode / preflight / reasoning overrides
   - context toggle overrides
2. `SettingsPage` configures provider transport details, but still lacks:
   - default prompt selection
   - global context toggle controls
3. The shipped prompt catalog is too thin:
   - only one default prompt is seeded
   - users have no prompt CRUD page yet
   - if an older config has `prompts: []`, enhancement becomes much weaker or unusable

## Design Decisions

### 1. Add surfaces only for primitives that already exist

Do not invent a second prompt or profile model in the UI. Use the existing config structure:

- `defaults.prompt_id`
- `defaults.context`
- `profiles[].overrides.*`

This keeps the frontend aligned with the engine/config resolution that already ships.

### 2. Prefer safe migration over hard validation

When the config has an empty prompt catalog, backfill the bundled prompts instead of rejecting the
config. The current product does not yet expose prompt CRUD, so an empty catalog is effectively a
broken state for enhancement.

### 3. Keep profile override semantics explicit

For profile override controls:

- booleans use `inherit / on / off`
- enums use `inherit / explicit value`
- string overrides use blank input plus a clear/reset affordance
- context toggles use per-field `inherit / on / off`

This matches the optional override semantics already modeled in `PowerModeOverrides`.

### 4. Do not pretend OCR is ready

`use_ocr` remains forward-compatibility state only. This batch will expose the actually useful text
context controls first and will not market OCR as working when it is not.

## Scope

### Settings

- Add default prompt selector
- Add global context toggles
- Keep provider, transport, and key management behavior unchanged

### Profiles

- Replace the stubbed override area with real controls for:
  - enhancement enable
  - prompt
  - LLM provider
  - API mode
  - preflight mode
  - reasoning effort
  - base URL
  - model
  - context toggles
- Also expose profile enabled/disabled state

### Defaults / migration

- Expand bundled prompt templates into a small starter catalog
- Backfill bundled prompts when an existing config loads with an empty prompt list

## Verification

- UI tests for:
  - settings prompt/context save
  - profile override save
  - profile selection still does not trigger refetch
- Rust tests for prompt backfill helper or related runtime behavior if needed
- Existing Rust/UI test suites still pass
- Live provider probes still pass after UI/config changes


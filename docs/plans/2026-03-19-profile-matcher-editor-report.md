# 2026-03-19 profile matcher editor report

## Scope

Close the remaining product gap in `ProfilesPage`: the config model already supported multiple matcher types, but the UI still behaved like a single process-name field.

This batch makes the standard design the user asked for actually usable:

- one profile can target multiple applications
- one profile can mix different matcher types
- process-name, executable-path, and window-title matching are all editable from the UI
- a tuned profile can be duplicated instead of rebuilt from scratch

## Problem

Before this batch, `ProfilesPage` only exposed one process-name equality field even though the underlying model already supported:

- `ProcessNameEquals`
- `ExePathEquals`
- `WindowTitleContains`
- multiple matchers per profile

That meant the repo had the capability in data/model code, but not in the actual product surface.

## Implementation

### Profiles UI

- `voicewin-tauri/src/ui/ProfilesPage.tsx`
  - replaced the single `Target application` field with a real matcher editor
  - each matcher row now has:
    - matcher type selector
    - matcher value input
    - row-level foreground capture
    - remove action
  - added quick-add buttons for:
    - `+ Process`
    - `+ Window Title`
    - `+ Executable`
  - added row-level `Use Foreground`
    - process matchers can capture foreground process name
    - executable matchers can capture foreground exe path
    - window-title matchers can capture foreground window title
  - added `Duplicate` profile action
    - clones the selected profile
    - assigns a fresh profile id
    - keeps matchers and overrides intact
  - sidebar summary now shows a compact matcher summary instead of only the first process matcher

### Tests

- `voicewin-tauri/src/ui/ProfilesPage.test.tsx`
  - added coverage for:
    - profile duplication
    - multiple matcher rows and matcher types
    - row-level foreground matcher capture

## Verification

Ran successfully:

- `npm test`
- `npm run typecheck`

Observed frontend test result after this batch:

- `ProfilesPage.test.tsx`: `6` tests passed
- full frontend suite: `11` files passed, `38` tests passed

## Outcome

Profiles now match the actual product model instead of the earlier MVP-only UI constraint.

That means VoiceWin now supports, from the real shipped UI:

- duplicate-and-retarget profile workflows
- one profile for multiple apps
- one profile for multiple windows/title variants
- mixed matcher strategies without editing config by hand

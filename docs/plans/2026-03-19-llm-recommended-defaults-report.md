# 2026-03-19 LLM recommended defaults report

## Scope

Tighten the shipped LLM setup so VoiceWin nudges users toward the path we actually validated:

- OpenAI-compatible should default to `gpt-5.4`
- OpenAI-compatible should default to `responses_sse`
- Chat Completions should stay available, but clearly as a legacy compatibility fallback
- Settings should expose a one-click way to apply the recommended provider stack

This is a product-defaults / ease-of-use batch, not a transport or engine rewrite.

## Why this batch

Most of the underlying LLM work is already done, but the shipped defaults still pointed at the older OpenAI-compatible path:

- model: `gpt-4o-mini`
- API mode: `chat_completions`

That created unnecessary friction because the real validated path in this repo is now:

- `gpt-5.4`
- `/v1/responses`
- HTTP SSE
- `store: false`
- optional reasoning, unset by default

So the UX was lagging behind the architecture and live validation.

## Decisions

- New-install defaults switch to `gpt-5.4` + `responses_sse`.
- `llm_preflight_mode` stays `off` by default.
- reasoning remains configurable but disabled by default.
- Chat Completions remains supported, but is now labeled `Chat Completions (Legacy)` in the UI.
- Existing configs are not force-migrated here; this batch improves defaults and adds an explicit recommended-stack action.
- `serde` compatibility defaults in `voicewin-core` were left alone to avoid surprising legacy configs that deserialize without explicit `llm_api_kind`.

Revision after follow-up completion on 2026-03-19:

- unchanged/custom configs still are not force-migrated broadly
- but the app now performs a narrow safe migration for the exact old built-in OpenAI default stack
- and also for exact old per-profile OpenAI override stacks
- that means untouched legacy configs upgrade automatically on load, while customized stacks still require explicit user action

## Implementation

### Runtime defaults

- `voicewin-runtime/src/defaults.rs`
  - `default_global_defaults()` now uses:
    - provider: `openai_compatible`
    - base URL: `https://api.openai.com/v1`
    - model: `gpt-5.4`
    - API mode: `responses_sse`
    - preflight: `off`
    - reasoning: `None`
  - added test `default_global_defaults_use_responses_and_gpt5_4`
  - added `migrate_legacy_openai_defaults_to_recommended(...)`
  - added `migrate_legacy_openai_profile_overrides_to_recommended(...)`
  - migration only triggers for the exact old built-in stack:
    - provider `openai_compatible`
    - base URL `https://api.openai.com/v1`
    - model `gpt-4o-mini`
    - API mode `chat_completions`
    - preflight `off`
    - reasoning unset
  - profile migration only triggers for exact old profile override stacks where the resolved provider is still OpenAI-compatible
- `voicewin-tauri/src-tauri/src/main.rs`
  - `load_or_init_config(...)` now persists that safe migration during normal config load

### UI helper defaults

- `voicewin-tauri/src/ui/llmConfig.ts`
  - OpenAI-compatible default model changed to `gpt-5.4`
  - invalid/missing OpenAI-compatible API mode now normalizes toward `responses_sse`
  - added `recommendedApiKindForProvider(...)`

### Settings UX

- `voicewin-tauri/src/ui/SettingsPage.tsx`
  - LLM section copy now explicitly says OpenAI-compatible defaults to Responses + `gpt-5.4`
  - added a legacy-stack callout for the older OpenAI default path
  - added `Apply Recommended` button from that callout
  - button applies:
    - provider-specific recommended API mode
    - provider-specific default base URL
    - provider-specific default model
    - `preflight = off`
    - reasoning disabled
  - OpenAI-compatible mode label now reads `Chat Completions (Legacy)`
  - OpenAI-compatible model example now reads `gpt-5.4`
- `voicewin-tauri/src/ui/SettingsPage.test.tsx`
  - updated label assertions
  - added test covering the legacy-stack callout
  - added test covering `Apply Recommended`

### Profiles UX

- `voicewin-tauri/src/ui/ProfilesPage.tsx`
  - updated API mode labels to `Chat Completions (Legacy)`
  - added a legacy-stack callout when a profile currently resolves to the older OpenAI stack
  - added `Apply Recommended Override` for per-profile modernization

### CLI / GUI consistency

- `voicewin-cli/src/main.rs`
  - default model -> `gpt-5.4`
  - default API mode -> `responses_sse`
  - default preflight -> `off`
- `voicewin-gui/src/main.rs`
  - default model -> `gpt-5.4`
  - default API mode -> `responses_sse`
  - default preflight -> `off`

### Test / smoke consistency

- `voicewin-runtime/src/config_store.rs`
  - round-trip fixture now uses a semantically correct Gemini stack
- `voicewin-tauri/src-tauri/src/runtime_smoke.rs`
  - deterministic runtime-smoke fixture now uses the recommended OpenAI stack

## Verification

Ran successfully:

- `cargo fmt --all`
- `cargo test -q -p voicewin-runtime`
- `cargo check -q`
- `cargo test -q`
- `npm test`
- `npm run typecheck`

## Relationship to prior live validation

This batch does not change the provider transport implementation itself.
It changes the shipped defaults and makes the validated path easier to choose.

The recommended OpenAI-compatible stack here is grounded in the earlier live validations already recorded in:

- `docs/plans/2026-03-19-llm-responses-sse-validation-report.md`
- `docs/plans/2026-03-19-provider-probe-report.md`
- `docs/plans/2026-03-19-llm-latency-benchmark-report.md`

## Outcome

VoiceWin now ships with product defaults that better match the path we actually validated and want users to adopt:

- OpenAI-compatible -> `gpt-5.4`
- OpenAI-compatible -> `responses_sse`
- one-click recommended-stack application in Settings
- legacy chat path still available when compatibility is needed

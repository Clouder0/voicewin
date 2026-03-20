# 2026-03-19 provider probe report

## Scope

Finish the lightweight LLM provider probe surface for the new post-processing stack:

- expose a Settings-surface probe using the current draft provider configuration
- use the stored provider-specific secret rather than raw key entry in the probe form
- keep it intentionally small and exact-output so it is useful for smoke checks
- measure and live-validate it against the real OpenAI-compatible Responses SSE gateway

## Product / UX decisions

- The probe is explicitly **not** a realistic prompt benchmark.
- The probe sends a tiny exact-output request and shows:
  - provider kind
  - API kind
  - model
  - total latency
  - first-token latency when available
  - final output text
- The button is disabled until the selected provider has a saved key.
- Probe results are cleared whenever the LLM draft changes so stale results do not linger.
- Settings copy now clearly says this is for quick smoke checks, not prompt-latency evaluation.

## Implementation

### New IPC / backend path

- `voicewin-runtime/src/ipc.rs`
  - added `ProviderProbeRequest`
  - added `ProviderProbeResponse`
- `voicewin-appcore/src/service.rs`
  - added `probe_llm_provider(...)`
  - added unit test `probe_llm_provider_returns_latency_and_output`
- `voicewin-tauri/src-tauri/src/main.rs`
  - added `probe_llm_provider` command and command registration

### Frontend

- `voicewin-tauri/src/lib/types.ts`
  - added probe request/response types
- `voicewin-tauri/src/ui/SettingsPage.tsx`
  - added provider probe row
  - added probe result / error state
  - wired probe to current LLM draft settings
- `voicewin-tauri/src/ui/SettingsPage.test.tsx`
  - added probe UI test covering invoke payload and rendered result

### Reproducible live benchmark helper

- `voicewin-appcore/examples/live_provider_probe.rs`
  - now supports `VOICEWIN_LIVE_ROUNDS`
  - now supports `VOICEWIN_LIVE_SLEEP_MS`
  - prints per-round provider latency plus min/avg/max summary

## Reliability hardening done while finishing this batch

While running the provider-probe test suite, a real isolation flaw showed up:

- `AppService` instances were sharing the global secret-store path indirectly
- concurrent tests could race and read the wrong API key file

Fix:

- `voicewin-runtime/src/secrets.rs`
  - added path-scoped helpers:
    - `set_secret_at_path(...)`
    - `get_secret_at_path(...)`
    - `delete_secret_at_path(...)`
- `voicewin-appcore/src/service.rs`
  - `AppService` now keeps its own `secrets_path`
  - service key reads/writes use the instance-scoped helpers
- `voicewin-runtime/src/runtime_engine.rs`
  - engine builder now receives the ElevenLabs key explicitly instead of re-reading the global store

This removed a real cross-test race and makes the appcore service layer more deterministic.

## Verification

### Rust / backend

Ran successfully:

- `cargo fmt --all`
- `cargo test -q -p voicewin-runtime`
- `cargo test -q -p voicewin-providers -p voicewin-engine -p voicewin-appcore`
- `cargo check -q -p voicewin-appcore --example live_provider_probe`
- `cargo check -q`
- `cargo test -q`
- `cargo check -q && cargo test -q` in `voicewin-tauri/src-tauri`

### Frontend

Ran successfully:

- `npm test`
- `npm run typecheck`

## Live end-to-end validation

Date: 2026-03-19

Real gateway used:

- base URL: `https://cc2.caaa.tech/v1`
- provider kind: `openai_compatible`
- API kind: `responses_sse`
- model: `gpt-5.4`
- transport: HTTP SSE via `/v1/responses`
- output target: exact text `VoiceWin provider probe ok.`

Command used:

```bash
env \
  VOICEWIN_LIVE_PROVIDER_KIND=openai_compatible \
  VOICEWIN_LIVE_BASE_URL=https://cc2.caaa.tech/v1 \
  VOICEWIN_LIVE_MODEL=gpt-5.4 \
  VOICEWIN_LIVE_API_KIND=responses_sse \
  VOICEWIN_LIVE_API_KEY=... \
  VOICEWIN_LIVE_ROUNDS=5 \
  cargo run -q -p voicewin-appcore --example live_provider_probe
```

### Run A: reasoning effort = `low`

Per-round provider timings:

- round 1: elapsed `3616 ms`, first token `3561 ms`
- round 2: elapsed `9587 ms`, first token `9408 ms`
- round 3: elapsed `1673 ms`, first token `1495 ms`
- round 4: elapsed `4068 ms`, first token `1611 ms`
- round 5: elapsed `1716 ms`, first token `1662 ms`

Summary:

- elapsed min/avg/max: `1673 / 4132 / 9587 ms`
- first-token min/avg/max: `1495 / 3547 / 9408 ms`
- final output: `VoiceWin provider probe ok.`

### Run B: reasoning disabled

Per-round provider timings:

- round 1: elapsed `2525 ms`, first token `2353 ms`
- round 2: elapsed `8368 ms`, first token `8253 ms`
- round 3: elapsed `3013 ms`, first token `2928 ms`
- round 4: elapsed `2771 ms`, first token `2702 ms`
- round 5: elapsed `2226 ms`, first token `2117 ms`

Summary:

- elapsed min/avg/max: `2226 / 3780 / 8368 ms`
- first-token min/avg/max: `2117 / 3670 / 8253 ms`
- final output: `VoiceWin provider probe ok.`

## Interpretation

- The probe path is working end to end against the real gateway.
- Exact-output correctness was stable in both 5-round batches.
- Latency variance is still large even with a tiny request.
- Because the repeated rounds reused a single `AppService` instance and its persistent HTTP client, I infer that connection reuse alone does **not** eliminate the tail; upstream queueing / model-side variability is still material.
- The probe is therefore useful for:
  - key validation
  - base URL / model / API-mode validation
  - fast smoke checks
- The probe should **not** be treated as the user-facing post-processing latency benchmark. The prompt-preview / real post-processing measurements remain the more relevant metric.

## Outcome

This batch is complete:

- provider probe UI and IPC path implemented
- tests green
- root compile/test green
- live real-gateway validation complete
- latency measurements collected and written down

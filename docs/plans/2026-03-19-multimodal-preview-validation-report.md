# Multimodal Preview Validation Report

Date: 2026-03-19

## Summary

VoiceWin's next multimodal slice is now implemented and validated for prompt preview and provider dispatch:

- screenshot/image artifacts can flow through preview context,
- OpenAI Responses SSE and Gemini native SSE request builders both accept attached images,
- the prompt preview UI can upload a sample screenshot and send it as a data URL,
- the appcore preview path now honors `use_ocr` so preview behavior matches runtime enhancement behavior,
- live probe and benchmark examples now accept optional image input for repeatable latency testing.

This batch was validated locally and against the user-provided live gateway:

- OpenAI Responses base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`
- OpenAI model: `gpt-5.4`
- Gemini model: `gemini-3-flash-preview`

## Code-Level Outcome

### Behavior fix

The preview path had a real correctness bug:

- runtime enhancement only attached screenshots when `context.use_ocr == true`,
- preview enhancement attached screenshots unconditionally.

This is now fixed in `voicewin-appcore` so preview and runtime use the same gate:

- `use_ocr = false` -> screenshot ignored
- `use_ocr = true` -> screenshot attached and `<SCREENSHOT_CONTEXT>` rendered

### New regression coverage

Added backend tests covering:

- preview attaches screenshot to OpenAI Responses requests when OCR is enabled,
- preview ignores screenshot when OCR is disabled.

### New live tooling

Extended example binaries so image-enabled validation is reproducible:

- `voicewin-providers/examples/live_responses_sse_probe.rs`
- `voicewin-providers/examples/live_gemini_sse_probe.rs`
- `voicewin-providers/examples/live_responses_latency_bench.rs`
- `voicewin-providers/examples/live_gemini_latency_bench.rs`
- shared helper:
  - `voicewin-providers/examples/support/mod.rs`

Supported env inputs:

- OpenAI examples:
  - `VOICEWIN_LIVE_IMAGE_PATH`
  - `VOICEWIN_LIVE_IMAGE_DATA_URL`
- Gemini examples:
  - `VOICEWIN_GEMINI_IMAGE_PATH`
  - `VOICEWIN_GEMINI_IMAGE_DATA_URL`
  - generic `VOICEWIN_LIVE_*` image vars also work

Extended appcore live preview examples:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `voicewin-appcore/examples/live_replay_preview.rs`

New preview env support:

- `VOICEWIN_LIVE_SCREENSHOT_DATA_URL`
- `VOICEWIN_LIVE_USE_OCR`

## Local Verification

Passed:

- workspace Rust tests:
  - `cargo test -q`
- Tauri Rust tests:
  - `cargo test -q` in `voicewin-tauri/src-tauri`
- targeted Rust regression lane:
  - `cargo test -q -p voicewin-appcore -p voicewin-runtime -p voicewin-providers`
- workspace Rust compile:
  - `cargo check -q`
- Tauri Rust compile:
  - `cargo check -q` in `voicewin-tauri/src-tauri`
- frontend tests:
  - `bun run test -- src/ui/PromptsPage.test.tsx src/ui/HistoryPage.test.tsx`
- frontend types:
  - `bun run typecheck`

## Live Artifact Used

Generated a deterministic local PNG for multimodal testing:

- file: `/tmp/voicewin-live-screenshot.png`
- size: `960x540`
- bytes: `14875`
- contents: centered text `VOICEWIN`

## Live End-to-End Validation

### Provider probe: OpenAI Responses SSE

Request shape validated live:

- `POST /v1/responses`
- `stream: true`
- `store: false`
- multimodal `input` with:
  - `input_text`
  - `input_image`
  - `detail: "low"`

Observed live result:

- HTTP `200 OK`
- returned text:
  - `VOICEWIN`
- `response.created` included a gateway `prompt_cache_key`

### Provider probe: Gemini native SSE

Request shape validated live:

- `POST /v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse`
- `parts` included:
  - text part
  - `inline_data` image part

Observed live result:

- HTTP `200 OK`
- returned text:
  - `VOICEWIN`

### Appcore preview: OpenAI Responses + screenshot

Realistic enhancer-style probe:

- prompt intent: use screenshot only to recover the exact product name
- transcript: `voice wen`

Observed live result:

- provider elapsed: `2322 ms`
- first token: `2186 ms`
- final output:
  - `VOICEWIN`

One intentionally unrealistic OCR-question probe was also run first:

- transcript: `what word is shown in the screenshot`
- result: `The word shown is **VOICEWIN**.`
- provider elapsed: `11110 ms`

Interpretation:

- the transport and screenshot path worked,
- but enhancer mode should be used for dictation cleanup with screenshot-assisted correction,
- not as a generic OCR question-answer mode.

### Appcore preview: Gemini native + screenshot

Same realistic enhancer-style probe:

- prompt intent: use screenshot only to recover the exact product name
- transcript: `voice wen`

Observed live result:

- provider elapsed: `2024 ms`
- first token: `2023 ms`
- final output:
  - `VoiceWin`

### Appcore overhead

In both live appcore preview runs, local service-layer overhead above provider time was negligible:

- OpenAI: `11115 - 11110 = 5 ms` on the first OCR-question run
- OpenAI realistic run: `2326 - 2322 = 4 ms`
- Gemini realistic run: `2029 - 2024 = 5 ms`

This means the current appcore preview path is not the latency bottleneck.

## Latency Benchmarks

Important:

- the `warmed` lane here is only `GET /models` connection warmup plus request reuse,
- it is **not** true prompt-cache/prefill reuse,
- so these numbers should inform conservative defaults, not aggressive cache assumptions.

### OpenAI Responses, low reasoning, transcript `voice wen`

| Variant | Cold median | Warmed median | Persistent median | First-token median | Clean outputs |
| --- | ---: | ---: | ---: | ---: | ---: |
| text only | 2735 ms | 4007 ms | 4317 ms | 2622 / 3770 / 4217 ms | 8/15 |
| image attached | 2564 ms | 1593 ms | 2995 ms | 2423 / 1481 / 2854 ms | 10/15 |

Observed behavior:

- text-only accuracy was unstable because the transcript was ambiguous without visual context,
- image attachment improved accuracy and improved warmed latency on this gateway,
- raw OpenAI outputs still occasionally became chatty even with image context, so post-processing remains necessary.

### Gemini native, low reasoning, transcript `voice wen`

| Variant | Cold median | Warmed median | Persistent median | First-token median | Clean outputs |
| --- | ---: | ---: | ---: | ---: | ---: |
| text only | 5334 ms | 5134 ms | 5142 ms | 5334 / 5134 / 5142 ms | 0/15 |
| image attached | 2317 ms | 1320 ms | 1627 ms | 2317 / 1320 / 1627 ms | 15/15 |

Observed behavior:

- without image context, Gemini consistently normalized `voice wen` into the wrong phrase (`Voice when`, once `Voice memo`),
- with image context, Gemini was perfect across all 15 sampled runs,
- Gemini image-warmed was the best multimodal latency result measured in this batch.

## Product Conclusions

### 1. The screenshot path is now solid for preview and provider dispatch

What is validated today:

- preview upload -> data URL -> backend override -> provider request
- OpenAI Responses multimodal request
- Gemini native multimodal request
- appcore prompt preview on both providers

What is still not implemented:

- automatic runtime screenshot capture from the platform layer
- OCR fallback extraction path for non-image-capable providers
- storing screenshot artifacts in history/session replay

### 2. `use_ocr` must remain the semantic gate

The preview/runtime mismatch was real and would have caused operator confusion.

Current correct behavior:

- screenshot context exists only when OCR/multimodal context is explicitly enabled,
- preview now reflects that exactly.

### 3. Do not change the latency default based on naive warmup alone

The earlier conservative default still stands:

- keep `llm_preflight_mode = off` by default

Reason:

- simple HTTP pre-connect via `/models` is not a reliable universal win,
- it helped Gemini image runs strongly,
- it helped some OpenAI image runs,
- but it was neutral-to-worse for OpenAI text-only on this gateway.

### 4. For screenshot-assisted correction on this gateway, Gemini is currently the strongest multimodal path

Based on this benchmark set:

- Gemini native + image had the best accuracy,
- Gemini native + image had the best warmed latency,
- OpenAI Responses + image is operationally viable and reasonably fast,
- but its raw output behavior is more variable and benefits more from VoiceWin post-processing.

That does not mean OpenAI support should be dropped.
It does mean the product should present Gemini as a strong first-choice option for multimodal correction.

## Recommended Next Steps

1. Implement real runtime screenshot capture behind the same `use_ocr` policy gate.
2. Keep post-processing strict for OpenAI multimodal enhancer runs.
3. Do not attempt speculative `prompt_cache_key` or `previous_response_id` activation by default.
4. If deeper cache work resumes, benchmark it as a separate batch with real stop-path timings rather than synthetic assumptions.

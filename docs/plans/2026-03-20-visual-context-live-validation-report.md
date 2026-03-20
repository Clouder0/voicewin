# 2026-03-20 Visual Context Live Validation Report

## Scope

Closed the remaining visual-context implementation work around:

- frontend migration from legacy `use_ocr` to:
  - `visual_context_mode`
  - `visual_capture_scope`
- legacy history rendering fallback so older payloads do not show `visual:undefined`
- live benchmark/example tooling updates so appcore examples can drive:
  - `off`
  - `auto`
  - `screenshot`
  - `ocr`
  - `display`
  - `foreground_window`

## Local Verification

Verified on March 20, 2026:

- `cargo fmt --all`
- `cargo check -q`
- `cargo check -q --examples -p voicewin-appcore`
- `cargo test -q`
- `npm run typecheck` in `voicewin-tauri`
- `npm test` in `voicewin-tauri`

All passed.

## Live Gateway Validation

Gateway under test:

- OpenAI-compatible base: `https://cc2.caaa.tech/v1`
- Gemini base: `https://cc2.caaa.tech/v1beta`

### 1. GPT-5.4 screenshot path via `/v1/responses`

Command path:

- `voicewin-appcore/examples/live_provider_probe.rs`
- `provider_kind=openai_compatible`
- `api_kind=responses_sse`
- `model=gpt-5.4`
- `probe_kind=screenshot_product_name`

Observed result:

- output: `VoiceWin`
- warnings: none
- rounds: 3
- elapsed min/avg/max: `1246 / 1846 / 2627 ms`
- first-token min/avg/max: `1144 / 1711 / 2501 ms`
- input tokens: `915`
- cached input tokens: `0`

Conclusion:

- GPT-5.4 screenshot input on `/v1/responses` is live-validated and working.

### 2. GPT-5.4 screenshot prompt-preview path

Command path:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `provider_kind=openai_compatible`
- `api_kind=responses_sse`
- `model=gpt-5.4`
- `visual_context_mode=screenshot`
- attached screenshot: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`

Observed result:

- screenshot attached and processed successfully
- rounds: 3
- elapsed min/avg/max: `2647 / 4117 / 6853 ms`
- provider elapsed samples: `2847, 6853, 2647 ms`
- provider first-token samples: `2774, 6741, 2451 ms`
- final output contained `VoiceWin`
- warning: none

Notes:

- The prompt-preview enhancer policy still biases toward transcript transformation, so the raw output was sometimes:
  - `The visible product name is **VoiceWin**.`
- That is a prompt-shape issue, not a screenshot transport failure.
- The provider probe above is the cleaner proof for screenshot transport correctness.

### 3. GPT-5.4 OCR mode prompt-preview path

Command path:

- `voicewin-appcore/examples/live_prompt_preview.rs`
- `provider_kind=openai_compatible`
- `api_kind=responses_sse`
- `model=gpt-5.4`
- `visual_context_mode=ocr`
- attached screenshot: `voicewin-appcore/assets/provider_probe_voicewin_640.data-url.txt`

Observed result:

- OCR sidecar executed successfully
- rounds: 3
- elapsed min/avg/max: `2325 / 4493 / 7967 ms`
- provider elapsed samples after OCR sidecar: `5281, 1501, 909 ms`
- provider first-token samples: `2232, 1395, 798 ms`
- main prompt contained:

```text
<SCREEN_OCR_TEXT>
VoiceWin
</SCREEN_OCR_TEXT>
```

- final output: `VoiceWin`
- warning: none

Conclusion:

- OCR mode is live-validated and working with GPT-5.4 on the real gateway.
- The implementation does not require a multimodal main request when the user explicitly selects OCR mode.

### 4. OpenAI-compatible text-only main path on this gateway

Attempted:

- `provider_kind=openai_compatible`
- `api_kind=chat_completions`
- `model=gpt-5.4`
- `model=gpt-4o-mini`

Observed result:

- both failed with `503 no_available_providers`
- provider details reported `format_type_mismatch`

Conclusion:

- This specific gateway is not currently a valid live target for OpenAI-compatible `chat_completions`.
- The OCR fallback implementation exists, but true text-only main-path validation on `cc2.caaa.tech` is blocked by provider availability, not by VoiceWin code.
- On this gateway, OpenAI-compatible should stay on `responses_sse`.

### 5. Gemini model compatibility

Attempted:

- `model=gemini-3.1-flash-preview`
- `api_kind=stream_generate_content_sse`

Observed result:

- failed with `503`
- body reported `未知模型，请检查模型名称是否正确`

Control validation:

- `model=gemini-3-flash-preview`
- smoke probe succeeded
- elapsed: `23825 ms`
- first token: `23824 ms`

Gemini screenshot probe:

- `model=gemini-3-flash-preview`
- `probe_kind=screenshot_product_name`
- succeeded with output `VoiceWin`
- elapsed: `2755 ms`
- first token: `2754 ms`

Conclusion:

- As of March 20, 2026, `gemini-3.1-flash-preview` is still not available on this gateway.
- `gemini-3-flash-preview` remains the working Gemini default on `cc2.caaa.tech`.

## Latency Benchmarks

Used:

- `voicewin-providers/examples/live_responses_latency_bench.rs`
- `provider=openai-compatible`
- `model=gpt-5.4`
- `api=/v1/responses`

### No image attached

- cold median total: `3128 ms`
- warmed median total: `927 ms`
- persistent median total: `2994 ms`
- warmed median first token: `761 ms`
- warmup request median: `618 ms`

### Image attached

- cold median total: `1904 ms`
- warmed median total: `1873 ms`
- persistent median total: `1734 ms`
- warmed runs were high-variance: `933 ms`, `1873 ms`, `3964 ms`

Interpretation:

- A lightweight pre-connect/warmup request can materially reduce latency in some runs, especially on the text-only no-image benchmark.
- The image path is more variable; a warmup call does not reliably guarantee a win.
- Persistent connection reuse alone was not consistently better than fresh warmed runs.
- Prompt/cache reuse is still worth pursuing, but it should be treated as an empirical optimization, not an assumed win.

## Implementation Notes

Additional implementation/tooling completed in this pass:

- updated `HistoryPage` to gracefully render older legacy context payloads
- updated appcore live examples to accept:
  - `VOICEWIN_LIVE_VISUAL_MODE`
  - `VOICEWIN_LIVE_VISUAL_CAPTURE_SCOPE`
- preserved legacy helper compatibility:
  - `VOICEWIN_LIVE_USE_OCR=true` still maps to `screenshot` for backward compatibility
- added UI recommendations in Settings and Profiles for:
  - `cc2.caaa.tech`
  - `openai_compatible`
  - `chat_completions`
  - one-click switch to `responses_sse` + `gpt-5.4`

## Product Recommendations

Current recommendation for this gateway:

- OpenAI-compatible default:
  - `responses_sse`
  - `gpt-5.4`
  - visual mode `auto` or `screenshot`
- OCR mode:
  - keep available as an explicit user choice
  - it is working and should remain exposed
- Gemini default on `cc2.caaa.tech`:
  - `gemini-3-flash-preview`
  - do not switch to `gemini-3.1-flash-preview` yet
- Chat Completions:
  - do not recommend on this gateway until a compatible provider is actually available

## Remaining Constraints

- Windows foreground-window screenshot capture is implemented best-effort.
- macOS foreground-window screenshot capture still falls back to full display capture and logs the fallback.
- Real text-only main-path OCR fallback could not be live-tested on `cc2.caaa.tech` because the gateway currently rejects OpenAI-compatible `chat_completions` requests regardless of model.

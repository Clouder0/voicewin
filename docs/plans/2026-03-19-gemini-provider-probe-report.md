# 2026-03-19 Gemini provider probe report

## Scope

Live-validate the new provider-probe path against Gemini on the real gateway so Gemini support is not just inferred from unit tests.

## Real gateway configuration

Date: 2026-03-19

- provider kind: `gemini`
- base URL: `https://cc2.caaa.tech/v1beta`
- model: `gemini-3-flash-preview`
- API kind: `stream_generate_content_sse`
- key: same gateway key used for the OpenAI-compatible validation
- probe target output: `VoiceWin provider probe ok.`

## Command used

```bash
env \
  VOICEWIN_LIVE_PROVIDER_KIND=gemini \
  VOICEWIN_LIVE_BASE_URL=https://cc2.caaa.tech/v1beta \
  VOICEWIN_LIVE_MODEL=gemini-3-flash-preview \
  VOICEWIN_LIVE_API_KIND=stream_generate_content_sse \
  VOICEWIN_LIVE_API_KEY=... \
  VOICEWIN_LIVE_ROUNDS=5 \
  cargo run -q -p voicewin-appcore --example live_provider_probe
```

## Results

### Reasoning disabled, 5 rounds

Per-round provider timings:

- round 1: elapsed `6990 ms`, first token `6989 ms`
- round 2: elapsed `22315 ms`, first token `22312 ms`
- round 3: elapsed `1963 ms`, first token `1963 ms`
- round 4: elapsed `18162 ms`, first token `18162 ms`
- round 5: elapsed `8242 ms`, first token `8214 ms`

Summary:

- elapsed min/avg/max: `1963 / 11534 / 22315 ms`
- first-token min/avg/max: `1963 / 11528 / 22312 ms`
- final output: `VoiceWin provider probe ok.`

### Reasoning `low`, 3 rounds

Per-round provider timings:

- round 1: elapsed `6821 ms`, first token `6819 ms`
- round 2: elapsed `2926 ms`, first token `2926 ms`
- round 3: elapsed `5894 ms`, first token `5894 ms`

Summary:

- elapsed min/avg/max: `2926 / 5213 / 6821 ms`
- first-token min/avg/max: `2926 / 5213 / 6819 ms`
- final output: `VoiceWin provider probe ok.`

## Interpretation

- Gemini support on the provided gateway is confirmed end to end through the real VoiceWin provider path.
- Exact-output correctness was stable in all live rounds.
- On this gateway, Gemini probe latency was highly variable and materially slower on average than the OpenAI-compatible `gpt-5.4` Responses path measured earlier.
- Gemini first-token latency is effectively the whole response latency in this exact-output probe, which suggests the gateway often delivers the useful output only near completion for this tiny request.
- The 3-round `low` reasoning sample was faster than the 5-round reasoning-disabled sample, but the sample is too small and noisy to treat as a reliable win.

## Outcome

Gemini remains a valid first-class provider in VoiceWin, but on this gateway today:

- OpenAI-compatible Responses + `gpt-5.4` looks like the better default latency path
- Gemini is supported and working, but should not be assumed faster without environment-specific measurement

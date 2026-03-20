# VoiceWin 0.2.0 release notes

## Highlights

VoiceWin `0.2.0` ships the first full LLM post-processing release on top of the ElevenLabs
scribe MVP.

This release adds:

- profile-aware LLM enhancement controls
- prompt selection plus bundled starter prompts
- OpenAI Responses SSE support with `gpt-5.4` as the recommended OpenAI-compatible path
- Gemini support in the LLM provider stack
- configurable reasoning, preflight, and provider/model/base URL overrides
- visual context modes including screenshot and OCR
- per-profile context toggles and app/profile matching improvements
- history/prompt preview/provider probe/benchmark tooling for latency and configuration review

## Recommended defaults

The product defaults now align with the stack validated in this repo:

- provider: OpenAI-compatible
- API mode: Responses SSE
- model: `gpt-5.4`
- reasoning: configurable, off by default
- preflight: off by default

Chat Completions remains available as a legacy compatibility path.

## Visual context

VoiceWin now supports explicit visual-context modes instead of a vague OCR-only toggle:

- `off`
- `auto`
- `screenshot`
- `ocr`

Capture scope is configurable, including full display and foreground-window oriented flows where
the platform supports them.

## Validation

This release was validated on the exact release candidate lineage with:

- local `bash scripts/ci/run-pr-checks.sh`
- native macOS packaged startup + runtime smoke
- native Windows packaged startup + runtime smoke

The release-candidate validation confirmed successful packaged insertion on both platforms and
closed the final native CI blockers discovered during release hardening.

# LLM Post-Processing Research

Date: 2026-03-18

## Goal

Understand how `voicewin` should evolve its "LLM post-processing" feature now that the ASR MVP is working, using:

- the current `voicewin` codebase,
- the bundled `VoiceInk` and `Handy` reference projects,
- current public product positioning from Typeless.

This note is intentionally product + architecture focused. It is meant to shape the next implementation batch, not to be a final spec.

## Short Conclusion

`voicewin` already has the right primitive seams for post-processing:

- prompt templates,
- enhancer vs assistant mode,
- trigger-word detection,
- context toggles,
- a distinct `enhancing` stage in the session pipeline,
- fallback to raw transcript when LLM enhancement fails.

What it does not have yet is the product surface around those primitives. Right now the feature exists mostly as hidden plumbing. VoiceInk and Handy both show that the hard part is not only "call an LLM after STT"; it is:

- prompt management,
- provider ergonomics,
- context capture,
- profile integration,
- observability/history,
- fast iteration and testing of prompts,
- and clear separation between cleanup, rewrite, and assistant behaviors.

The correct next move is not "add more prompting." The correct next move is to turn the existing plumbing into a coherent post-processing subsystem.

## What `voicewin` Already Has

### Core/domain

`voicewin` already models most of the important concepts:

- `PromptTemplate`, `PromptMode`, trigger words, and prompt building in `voicewin-core/src/enhancement.rs`
- context toggles in `voicewin-core/src/context.rs`
- profile/default override resolution in `voicewin-core/src/power_mode.rs`
- enhancement output filtering in `voicewin-core/src/text.rs`

Important existing behavior:

- trigger words can auto-enable enhancement and switch prompts for the current session
- trigger words are stripped from the dictated text before insertion
- enhancer and assistant modes are already distinct
- context blocks are already designed as structured tags:
  - `<TRANSCRIPT>`
  - `<CURRENTLY_SELECTED_TEXT>`
  - `<CLIPBOARD_CONTEXT>`
  - `<CURRENT_WINDOW_CONTEXT>`
  - `<CUSTOM_VOCABULARY>`

### Runtime/engine

The engine pipeline is already shaped correctly:

- `transcribe -> optional enhance -> insert`
- enhancement failure falls back to raw transcript instead of failing the whole session
- timings are tracked separately for transcription and enhancement
- realtime STT can still reuse the same post-STT pipeline

Relevant files:

- `voicewin-engine/src/engine.rs`
- `voicewin-engine/src/session.rs`
- `voicewin-runtime/src/llm.rs`

### UI/runtime state

The app already exposes:

- enhancement on/off,
- OpenAI-compatible base URL,
- model,
- secret storage for API key,
- an `enhancing` overlay stage.

That is enough for MVP plumbing, but not enough for a user-facing feature.

## Where `voicewin` Is Still Thin

### 1. Prompts exist in config, but not as a product surface

The config supports `prompts: Vec<PromptTemplate>`, but the app has no real prompt library UX yet:

- no prompt create/edit/delete UI
- no duplicate/test workflow
- no prompt picker in settings
- no prompt override UI in profiles
- no quick prompt switching during a session

This means the core model is ahead of the product.

### 2. Context model is ahead of platform capture

The engine supports:

- selected text
- clipboard
- window context
- custom vocabulary

But the platform implementations are still partial:

- Windows currently provides window context + clipboard
- macOS currently provides only window context
- selected text is not wired in either platform path
- custom vocabulary is not implemented as a real source yet
- OCR is explicitly deferred

So today the prompt builder supports more than the runtime can actually feed.

### 3. History is too lossy

Current `HistoryEntry` stores only:

- final text,
- stage,
- app metadata,
- error.

That is not enough for a serious enhancement workflow. It loses:

- raw transcript,
- enhanced text,
- prompt used,
- trigger word used,
- provider/model used,
- timings,
- warnings,
- request payload/debug material.

VoiceInk and Handy both preserve much richer artifacts. `voicewin` currently cannot support "re-run enhancement" or serious prompt debugging without redesigning history/session persistence.

### 4. LLM provider support is still "single hard-coded path"

Current runtime support is:

- one OpenAI-compatible provider abstraction,
- one global API key,
- one base URL,
- one model.

Missing:

- provider registry / multiple LLM endpoints
- connectivity tests
- model discovery
- provider-specific parameters
- local/on-device provider options
- provider-specific error handling surfaced to users

### 5. Profile integration is structurally present but UX-incomplete

Profiles already support overriding:

- enhancement on/off
- prompt id
- base URL
- model
- context toggles

But `ProfilesPage` explicitly says override UI is stubbed. This is a major blocker because per-app prompting is one of the highest-value use cases.

### 6. There is no "prompt lab"

Without a local test harness, prompt development will be slow and noisy. Users need:

- sample text input
- run enhancement without recording
- inspect raw transcript vs output
- compare prompts
- see why a prompt misbehaved

Right now the only realistic way to iterate is to actually dictate, which is too expensive.

## What VoiceInk Does That Matters

VoiceInk is the closest architectural reference because it treats enhancement as a first-class subsystem, not just a single API call.

### 1. It captures context before enhancement, not during paste-time recovery

VoiceInk captures clipboard and screen/window context at recording start, in the background, before enhancement runs. That is important because context should describe the user's editing environment at dictation time, not after the app has already started mutating clipboard/paste state.

This is a strong pattern for `voicewin`.

### 2. It clearly separates prompt semantics

VoiceInk distinguishes:

- transcription enhancement
- assistant mode

And it wraps normal prompts in a strong system template unless the prompt opts out. This is valuable because most users do not actually want raw-prompt freedom. They want a safe default that keeps the model in "rewrite, do not chat" mode.

### 3. It treats trigger words as ephemeral session overrides

VoiceInk:

- detects trigger words,
- enables enhancement if needed,
- switches prompts for that session,
- strips the trigger word,
- restores previous prompt/enhancement state afterward.

`voicewin` already mirrors the core logic, which is a good sign. The missing part is productization.

### 4. It stores the artifacts needed for debugging

VoiceInk persists:

- raw transcript,
- enhanced text,
- prompt name,
- enhancement model,
- enhancement duration,
- request messages sent to the AI provider.

That is a major reason its enhancement system is operable. When users say "this prompt is bad" or "the AI did something weird," there is something concrete to inspect.

### 5. It includes retry/backoff and provider-specific behavior

VoiceInk includes:

- retry with exponential backoff,
- provider-specific headers/body handling,
- filtering of `<thinking>`, `<think>`, `<reasoning>` blocks,
- optional `reasoning_effort` for models that support it.

That is a useful implementation checklist for `voicewin`'s provider layer.

### 6. It ships a prompt library, not just a text box

VoiceInk has predefined prompts like:

- Default
- Assistant
- Chat
- Email
- Rewrite

This matters because it teaches the feature. Users learn what the system can do by seeing the prompt catalog.

## What Handy Does That Matters

Handy is a simpler but useful cross-platform reference.

### 1. It treats post-processing as a replaceable layer after transcription

That sounds obvious, but it keeps the architecture clean:

- transcription produces raw output
- post-processing optionally transforms it
- history stores both

For `voicewin`, this validates keeping enhancement as a distinct pipeline stage instead of mixing it into STT provider logic.

### 2. It stores both original and post-processed text

That is the minimum viable persistence model for a usable post-processing feature.

### 3. It has provider/model/prompt management surfaces

Handy supports:

- provider selection
- per-provider API keys
- model fetching
- prompt CRUD
- selected prompt

Its prompt system is simpler than VoiceInk's, but the management loop is already user-real.

### 4. It shows a path for optional local LLM providers

Handy includes Apple Intelligence as a provider on supported Apple hardware. The broader lesson is:

- do not hard-code cloud-only assumptions into the product model
- keep "provider" and "model source" abstract

That is especially important if `voicewin` wants to stay cross-platform.

## What Typeless Changes About The Product Bar

Typeless raises the expectation above "transcript cleanup."

Its public positioning is closer to:

- intelligent dictation
- automatic filler/repetition removal
- auto-editing when the speaker changes their mind
- auto-formatting lists/steps
- per-app tone/style adaptation
- personal dictionary
- selected-text editing with voice commands

This implies an important product distinction:

### Dictation cleanup

The user just wants the text to come out well by default.

Examples:

- remove fillers
- normalize punctuation
- keep only the final intended phrasing
- structure a spoken list

### Prompted rewrite

The user wants a style or output transform.

Examples:

- "rewrite"
- "email"
- "chat"
- "make this concise"

### Assistant / edit mode

The user wants the selected text or transcript treated like an instruction target.

Examples:

- shorten selected text
- explain selected text
- rewrite selected text in another tone
- answer a question about selected content

`voicewin` should not collapse all three into one toggle. They are different user intents and should become different product surfaces over time.

## Design Implications For `voicewin`

### 1. Treat post-processing as a text pipeline, not a single checkbox

Recommended conceptual pipeline:

1. Raw transcript
2. Deterministic cleanup
3. Optional LLM transformation
4. Final insertion text

Why:

- deterministic cleanup is fast, cheap, and predictable
- it reduces noise before the LLM sees the text
- it improves behavior even when no LLM is configured

`voicewin` already has a basic deterministic filter. That should become more intentional rather than remaining a hidden heuristic.

### 2. Split feature modes explicitly

Recommended user-facing modes:

- `Cleanup`
  - default dictation cleanup
  - preserve intent
  - no stylistic transformation beyond normalization
- `Rewrite`
  - prompt-driven output style
  - email/chat/rewrite/etc.
- `Assistant`
  - instruction-following text transformation or selected-text operation

This split should exist in both:

- prompt metadata
- UI copy

`PromptMode::Enhancer` and `PromptMode::Assistant` already point in this direction. The product should now follow the model.

### 3. Redesign history around session artifacts

Before expanding UX, `voicewin` should stop persisting only the final text.

Recommended history/session artifact fields:

- raw transcript
- final inserted text
- enhanced text, if any
- stage outcome
- app identity
- STT provider/model
- LLM provider/model
- prompt id/title
- detected trigger word
- transcription/enhancement timings
- warning/error summary
- context flags used

Nice-to-have later:

- exact system/user messages sent to the LLM
- redacted provider response metadata

Without this, prompt iteration and bug triage stay weak.

### 4. Make context capture a first-class subsystem

Recommended rule:

- capture context near recording start
- freeze it for the session
- feed the frozen snapshot into enhancement

Priority order for cross-platform usefulness:

1. window/app context
2. clipboard context
3. selected text
4. custom vocabulary
5. OCR/screen text

Do not wait for OCR. The first four already give a strong system.

### 5. Add a real provider abstraction

Recommended near-term scope:

- keep OpenAI-compatible as the common baseline
- allow multiple named LLM providers/endpoints
- store provider selection separately from model
- add connectivity test
- add model listing when endpoint supports `/models`

Recommended later:

- local providers
- provider-specific advanced parameters
- explicit retry/backoff policy

### 6. Productize prompts as a library

Recommended predefined prompts for first usable release:

- Default Cleanup
- Rewrite
- Email
- Chat
- Assistant

Each prompt should have:

- title
- mode
- instructions
- optional trigger words
- short description

Nice-to-have later:

- icon
- examples
- "strict cleanup" vs "more polished"

### 7. Build a prompt lab before expanding complexity

Minimum useful tooling:

- sample input text area
- prompt selector/editor
- run button
- raw output + filtered output
- context preview
- copy result

This should be done before trying to support many prompts/providers. Otherwise prompt work will stay blind.

## Recommended Implementation Order

### Phase 1: Make the existing feature real

Ship the minimum complete user-facing system:

- prompt management UI
- default prompt selection
- prompt selection in settings
- prompt override in profiles
- richer history model with raw + enhanced + final text
- provider connectivity test
- model listing for OpenAI-compatible endpoints

This is the highest leverage batch because most of the engine plumbing already exists.

### Phase 2: Strengthen context and debuggability

- capture clipboard on macOS too
- add selected text capture on both platforms where feasible
- add custom vocabulary source
- persist prompt/provider/timing metadata in history
- add "re-run enhancement" from history
- add prompt lab / sample-text testing

### Phase 3: Introduce intent-specific flows

- distinct Cleanup vs Rewrite vs Assistant UX
- predefined prompt catalog
- better trigger-word UX
- assistant flow for selected text
- per-profile prompt/provider/context presets

### Phase 4: Reach for Typeless-level polish

Only after the foundation above:

- stronger deterministic cleanup
- smarter list/structure formatting
- better correction of self-repairs
- selected-text voice editing workflows
- translation-oriented post-processing
- optional personalization / adaptive tone

Important: Typeless-level "intelligent dictation" is not just a prompt. It is a product behavior built across ASR, text cleanup, context, personalization, and per-app policy. `voicewin` should not pretend it can clone that with one more LLM call.

## Concrete Advice For The Next Implementation Batch

If the goal is one practical next feature batch, I would build exactly this:

1. Prompt library page
   - CRUD for prompts
   - choose default prompt
   - trigger word editing
   - enhancer vs assistant mode

2. Better history schema
   - raw transcript
   - enhanced text
   - final text
   - prompt title/id
   - provider/model
   - timings

3. Profile override UI
   - enable enhancement
   - select prompt
   - select LLM provider/model
   - context toggles

4. Provider testing
   - test endpoint
   - test auth
   - fetch models

5. Prompt lab
   - sample transcript
   - run against current provider
   - inspect output

This would make the feature genuinely shippable without overreaching.

## What I Would Not Do Next

- Do not add more hidden prompt logic before building prompt management.
- Do not rely on OCR/screen capture as a prerequisite; it is useful but not required for the next release.
- Do not store only final text in history any longer.
- Do not merge assistant use cases into ordinary cleanup prompts.
- Do not promise Typeless-style "AI auto editing in real time" yet. Your current architecture is still stop-time enhancement, and that is fine.

## Sources

Internal code references:

- `voicewin-core/src/enhancement.rs`
- `voicewin-core/src/context.rs`
- `voicewin-core/src/power_mode.rs`
- `voicewin-core/src/text.rs`
- `voicewin-engine/src/engine.rs`
- `voicewin-engine/src/session.rs`
- `voicewin-runtime/src/llm.rs`
- `voicewin-runtime/src/history.rs`
- `voicewin-platform/src/windows.rs`
- `voicewin-platform/src/macos.rs`
- `voicewin-tauri/src/ui/SettingsPage.tsx`
- `voicewin-tauri/src/ui/ProfilesPage.tsx`
- `voicewin-tauri/src/ui/HistoryPage.tsx`
- `voicewin-appcore/src/service.rs`
- `VoiceInk/VoiceInk/Services/AIEnhancement/AIEnhancementService.swift`
- `VoiceInk/VoiceInk/Services/PromptDetectionService.swift`
- `VoiceInk/VoiceInk/Models/AIPrompts.swift`
- `VoiceInk/VoiceInk/Models/PromptTemplates.swift`
- `VoiceInk/VoiceInk/Whisper/WhisperState.swift`
- `Handy/src-tauri/src/actions.rs`
- `Handy/src-tauri/src/llm_client.rs`
- `Handy/src-tauri/src/settings.rs`

External references:

- Typeless home page: https://www.typeless.com/
- Typeless Windows beta release notes: https://www.typeless.com/help/release-notes/windows/introducing-typeless-windows-app-beta
- Typeless "Speak to edit" release notes: https://www.typeless.com/help/release-notes/ios/speak-to-edit
- Typeless translation mode release notes: https://www.typeless.com/help/release-notes/macos/translation-mode
- Typeless personalization release notes: https://www.typeless.com/help/release-notes/macos/personalized-smarter

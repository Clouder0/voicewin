# VoiceWin GUI Design Requirements

This document is a functional design brief for the VoiceWin application UI/UX.

It is intended as a hand-off to a professional designer.

- Scope: user goals, product surfaces, information architecture, flows, states, configuration domains, edge cases, and acceptance criteria.
- Out of scope: visual style, colors, typography, spacing, exact layouts, and implementation details.

## 1) Background & Product Goal

VoiceWin is a system-level voice-to-text utility that lets a user dictate into any application with minimal friction.

What the product replaces:
- switching to a dedicated dictation app,
- copy/paste shuffling between apps,
- reformatting dictated text manually,
- remembering different "tones" for different contexts.

Core promise:
- Trigger recording from anywhere.
- Speak.
- Stop.
- Text appears where the user is currently typing.

The UI must support both "quick capture" and "careful control":
- quick capture: minimal steps, minimal UI interruption, strong status feedback.
- careful control: explicit configuration, per-app behaviors, testing tools, and troubleshooting.

VoiceWin must support:
- offline-first workflows (local transcription),
- optional cloud transcription (user choice),
- optional text enhancement (rewrite/format/fix tone),
- per-application behavior (scenario profiles),
- reliable insertion/pasting across many target apps.

## 2) Target Users & Usage Context

VoiceWin is a background utility used while the user is already focused on another task.

Typical usage contexts:
- chat apps (short messages, informal tone)
- email clients (structured, formal tone)
- docs and note-taking (long-form dictation)
- IDEs and terminals (special paste behaviors; sensitive to key injection)
- meetings (capturing snippets quickly)

User priorities (what the experience must optimize for):
- speed: minimal steps to start/stop and get text inserted
- trust: clear feedback that audio is captured and work is progressing
- control: per-scenario configuration without requiring technical knowledge
- safety: secrets protected; clear privacy controls
- recovery: when something fails, the user can fix it quickly

## 3) Target Platforms & Constraints

Primary target:
- Windows 11 (64-bit), non-admin user accounts.

Constraints that must be reflected in UX:
- Users may lack prerequisites (e.g., missing model files, blocked microphone access).
- Input devices can change (Bluetooth, unplug/plug).
- Some apps block or behave differently with paste (terminals, elevated apps, secure fields).
- The app will often be used while the user is focused on another application; feedback must not require context switching.
- Some environments are locked down (corporate devices, restricted installs). The UI must guide workarounds.

## 4) Experience Principles (Functional)

These are behavioral requirements, not visual style requirements.

- The overlay and shortcuts are the primary UX; the main window is for setup and management.
- Every step in the session pipeline must have clear feedback and a user action (cancel, retry, open fix).
- Users should never need to "guess" whether recording is active.
- Users should never need to re-open the app to recover from common failures (mic unplugged, provider missing, paste fails).
- Advanced features (profiles, multi-provider, model library) must be discoverable without being forced on first-time users.
- All diagnostic and export features must avoid leaking secrets.

## 5) Definitions (Glossary)

- Session: one end-to-end run from start recording to final insertion (or cancel/error).
- Transcript:
  - raw transcript: output of speech-to-text.
  - enhanced text: optional transformation of the transcript.
  - inserted text: what is actually pasted into the target app.
- STT provider: speech-to-text backend (local or cloud).
- STT model:
  - local model file(s) on disk, or
  - remote model identifier(s) for cloud providers.
- LLM provider: text enhancement backend (typically cloud, possibly local).
- Prompt template: reusable enhancement instructions (with optional trigger words).
- Scenario profile: a per-application configuration activated by the currently focused app.
- Insertion method: the specific technique used to paste/insert text into the target app.
- History: saved record of past session outputs and associated metadata.

## 6) Core User Journeys

### 6.1 Primary Journey: Dictate Into Any App

User intent: "I am typing somewhere and want to dictate." 

Functional steps:
1) Start recording (hotkey or alternate trigger).
2) Speak.
3) Stop recording.
4) Transcribe.
5) Optional enhancement.
6) Insert into the active app.

User-visible requirements:
- Start recording via global hotkey (and optionally other triggers).
- Immediate confirmation that recording started.
- A mic activity indicator (level/confidence) during recording.
- Stop recording quickly and reliably.
- Progress feedback for transcribing/enhancing/inserting without requiring the main window.
- Insertion should be reliable or fail with a clear reason and next step.
- Provide a quick way to copy the last result and re-insert it.

Recovery requirements:
- If the microphone is missing or blocked, present a specific fix path.
- If transcription fails (model missing, provider auth missing, network), present a fix path.
- If insertion fails (target app blocks paste), present a fix path and allow changing insertion method.

### 6.2 Setup Journey: First Run (Onboarding)

User intent: "I just installed it and need it to work." 

Onboarding must ensure the system is operational before the user tries it in a real app.

Minimum onboarding checklist (design target):
1) Activation
   - choose toggle vs push-to-talk behavior
   - set one or more hotkeys
   - configure cancel behavior
2) Microphone
   - select default mic or a named mic
   - run a mic test (level meter + brief recording playback or sample capture)
   - explain microphone privacy permissions and how to fix them
3) Transcription setup
   - choose default STT provider (local or cloud)
   - choose language behavior (auto vs explicit)
4) Model setup (for local STT)
   - install or import a model
   - validate model integrity
   - optionally download recommended models
5) Provider setup (for cloud STT and/or enhancement)
   - securely store API keys
   - test connectivity and show clear failure reasons
6) Insertion setup
   - choose insertion method and clipboard restore behavior
   - run an insertion test in the currently focused app
7) Enhancement setup (optional)
   - enable/disable by default
   - pick default prompt
   - test prompt on sample text
8) Privacy controls
   - history on/off and retention
   - clarify what data leaves the device for chosen providers

### 6.3 Tuning Journey: Per-App Profiles (Scenarios)

User intent examples:
- "In chat apps I want short informal text."
- "In email clients I want formal, well-structured text."
- "In terminals I need a different paste method." 

Requirements:
- Create profiles that activate based on the current foreground application.
- Provide a "capture current app" helper to reduce manual matching effort.
- Profiles can override:
  - STT provider and model
  - language
  - enhancement on/off
  - enhancement prompt
  - insertion method
  - history retention
  - context sources (clipboard/selected text/window context)

### 6.4 Review Journey: History & Reuse

User intent: "I want to find what I just dictated." 

Requirements:
- View a list of past sessions with timestamps.
- Provide quick copy and re-insert actions.
- Provide filtering/search.
- Allow deletion of one entry or clearing all.
- Allow history to be disabled.

### 6.5 Support Journey: Troubleshooting

User intent: "It failed and I need to fix it or report it." 

Requirements:
- A Diagnostics area that:
  - shows readiness and last error,
  - provides simple self-tests,
  - provides a copy/exportable diagnostic bundle for support,
  - never includes secrets.

## 7) Product Surfaces (UI Contexts)

VoiceWin should be designed as a background utility with multiple surfaces.

### 7.1 Overlay HUD (Primary Runtime Surface)

Purpose:
- Provide in-context feedback while the user remains inside other apps.

Functional requirements:
- Always communicates the current session state and what it means.
- Shows a recording timer when recording.
- Shows mic activity/level during recording.
- Shows progress during processing stages.
- Shows a last-result preview after completion (when safe and appropriate).

Interaction requirements:
- Minimal controls:
  - cancel session,
  - copy last output,
  - re-insert last output,
  - open main window,
  - optional mode switch (toggle vs push-to-talk).
- Supports repositioning and persistence of its position.

Behavioral requirements:
- The overlay must not steal focus from the user’s target application.
- The overlay must be readable at a glance, with clear state transitions.
- Errors must be actionable: each error should indicate the next best user action.

### 7.2 Tray/Menu Bar Surface

Purpose:
- Quick access and status without opening the main window.

Requirements:
- Start/Stop recording.
- Cancel.
- Open main window.
- Open History.
- Open Diagnostics.
- Quit.
- Optionally: "paste last result" and "paste last enhanced" actions.

### 7.3 Main Window (Configuration + Management)

Purpose:
- Setup, tuning, model/provider management, and history.

Top-level areas required:
- Record/Overview
- Models
- Providers
- Enhancement (Prompts)
- Profiles (Scenarios)
- Insertion
- History
- Diagnostics

## 8) Information Architecture (Designer Target)

The designer should assume the user spends most of their time outside the main window.

Information architecture goals:
- Make the “record → insert” loop obvious and confidence-inspiring.
- Make prerequisites (mic, model, provider) discoverable and easy to fix.
- Make advanced configuration (profiles, multiple models/providers) powerful but safe.
- Make troubleshooting fast and self-serve.

Recommended IA properties:
- Clear separation between:
  - Operation (recording loop + last result)
  - Setup (mic/models/providers)
  - Automation (profiles)
  - Output (history)
  - Support (diagnostics)


The designer should propose a structure that separates:
- runtime operation (recording loop and overlay)
- configuration and management (main window)
- troubleshooting (diagnostics)

Required top-level areas (names can change):
- Record / Overview
- Models
- Providers
- Enhancement (Prompts)
- Profiles (Scenarios)
- Insertion
- History
- Diagnostics

Key requirement: reduce cognitive load.
- New users should be able to use the app after completing onboarding, without understanding all advanced areas.
- Power users should be able to manage multiple providers and multiple models, and configure profiles safely.

## 9) Functional Modules (What must be configurable)

### 9.1 Activation & Shortcuts

Requirements:
- One or more global hotkeys.
- Support at least:
  - toggle recording,
  - push-to-talk (press and hold),
  - cancel.
- Hotkey conflict detection and resolution guidance.
- Optional alternate triggers (mouse button, tray click) if feasible.

### 9.2 Microphone Management

Requirements:
- Select microphone: default device or a named device.
- Show current selected microphone and whether it is available.
- Provide a mic test tool (level meter + record short sample).
- Provide guidance for microphone privacy restrictions.

### 9.3 STT Providers (Multiple)

VoiceWin must support multiple transcription providers.

Requirements:
- The user can choose a default STT provider.
- The system supports multiple providers that can be configured and tested.
- Provider selection can be overridden per profile.

Per-provider configuration requirements:
- Local provider:
  - model selection (from local model library),
  - language handling,
  - resource considerations (CPU, disk, startup time).
- Cloud providers:
  - API key management (stored in secure key storage),
  - optional endpoint/base URL,
  - model name/identifier selection,
  - test button to validate credentials and connectivity,
  - clear explanation of what audio is sent and when.

Provider UX requirements:
- A provider should be "configured" or "not configured" with explicit signals.
- A provider test should yield a clear, user-actionable result (not raw error dumps).

### 9.4 Model Management (Multiple Models)

VoiceWin must support a model library experience that can scale beyond one model.

Requirements:
- Show currently selected default model.
- Maintain a library of installed local models.
- Support:
  - download recommended models (with progress),
  - import local models from a file,
  - delete/uninstall models,
  - validate models (integrity checks),
  - show model disk usage and location.

Multi-model selection requirements:
- Users can select a default transcription model.
- Users can switch models quickly and understand tradeoffs (speed/quality/disk usage) without deep technical knowledge.
- Profiles can override the model selection.

Advanced requirements (optional but valuable):
- "Warm-up" indicator or readiness state for selected model.
- Safe upgrades: prevent partial/corrupt model installs from becoming the default.

### 9.5 Enhancement (LLM + Prompts)

Requirements:
- Global enhancement default: on/off.
- Prompt library:
  - create/edit/delete/duplicate prompts,
  - name/title,
  - mode ("enhancer" vs "assistant"),
  - trigger words (optional),
  - test prompt on sample text.

Trigger-driven behavior:
- If a trigger is detected in the dictated text, the system may:
  - choose a specific prompt,
  - enable enhancement for that session,
  - strip the trigger word from the final output.

### 9.6 LLM Providers (Multiple endpoints)

Minimum requirement:
- Support OpenAI-compatible endpoints via configurable base URL and model.

Requirements:
- API key management via secure storage.
- Test connectivity.
- Show last error (rate limits, auth, timeouts) in a user-readable way.

### 9.7 Scenarios / Profiles (Per-App Behavior)

A profile is activated when the current foreground application matches its rules.

Requirements:
- Match rules:
  - process name,
  - executable path,
  - window title pattern.
- A helper to capture the currently focused app and prefill match rules.
- Conflict handling when multiple profiles match:
  - priority ordering,
  - explicit resolution rules.
- Overrides:
  - STT provider, model, language,
  - enhancement on/off,
  - selected prompt,
  - insertion method,
  - history retention,
  - context toggles.

### 9.8 Insertion/Pasting

Insertion must be reliable across many apps.

Requirements:
- Configurable insertion method options including:
  - standard paste,
  - paste + enter,
  - terminal-friendly paste method(s).
- Clipboard restore options and delay.
- A "Test paste" tool that:
  - pastes a sample string into the current app,
  - verifies user sees it,
  - restores clipboard if enabled.
- Clear warnings for contexts where insertion may be blocked (e.g., elevated apps).

### 9.9 History

Requirements:
- Enable/disable history.
- Display entries with metadata:
  - timestamp,
  - target app identity,
  - stage outcome,
  - raw transcript and enhanced output (if available),
  - provider/model used (optional but valuable).
- Actions:
  - copy,
  - re-insert,
  - re-run enhancement.
- Retention controls:
  - max entries and/or days (optional).

### 9.10 Diagnostics

Requirements:
- Show readiness summary:
  - microphone present and permitted,
  - default provider configured,
  - model availability,
  - key presence (never show raw keys),
  - last session status.
- Self-tests:
  - mic capture test,
  - provider connectivity test,
  - insertion test,
  - model integrity check.
- Export support bundle:
  - log folder path,
  - version info,
  - last error summary,
  - configuration summary with secrets removed.

## 10) Data & Privacy Requirements

Requirements:
- Clear control over what data is stored:
  - history on/off,
  - ability to clear history,
  - retention settings (optional).
- Clear control over what data leaves the device:
  - cloud STT provider selection,
  - cloud LLM provider selection,
  - explicit disclosure of what is sent and when.
- Secrets must be stored in OS secure storage.
- Diagnostics exports must redact secrets.

## 11) Non-Functional Requirements (UX-impacting)

- Reliability: start/stop recording must never hang.
- Recoverability: failures must guide the user to fixes without restarting.
- Responsiveness: overlay must remain responsive even while processing.
- Performance: avoid unnecessary background CPU usage.
- Accessibility: keyboard navigation and screen reader labels for main window.

## 12) Acceptance Criteria (User-Level)

A design meets requirements if a typical user can:
- Install, complete onboarding, and successfully dictate into a third-party app within minutes.
- Use the app primarily via hotkey/overlay without needing to keep the main window open.
- Understand the current state (recording/transcribing/enhancing/inserting) at a glance.
- Switch microphones, run a mic test, and verify capture.
- Install/import multiple models, select a default model, and switch models confidently.
- Configure multiple providers, understand privacy implications, and validate setup via tests.
- Define at least two per-app profiles that reliably apply different prompts and insertion behaviors.
- Retrieve past transcripts, copy them, re-insert them, and optionally re-run enhancement.
- When errors occur, quickly find the fix path and/or provide diagnostics to support without exposing secrets.

## 13) Designer Deliverables Requested

The designer should produce:
- Information architecture (navigation and grouping of modules).
- User flows for onboarding, recording loop, model management, profile creation, and troubleshooting.
- Wireframes for overlay, tray menu, main window, model manager, profile editor, and diagnostics.
- Interaction specs for state transitions, confirmations, and error recovery.
- UX copy guidelines for errors, permissions, privacy prompts, and provider/model explanations.
- A “first-run success” checklist the user can complete.

## Appendix A) User Stories (Concrete)

These user stories are intended to keep the design grounded in real use.

- As a user, I can press a hotkey, speak, press again, and the text appears in the app I’m typing in.
- As a user, I can see a clear indication that the microphone is capturing audio.
- As a user, I can cancel a session immediately.
- As a user, I can choose a different microphone and confirm it works.
- As a user, I can install multiple transcription models, select a default model, and switch models per scenario.
- As a user, I can configure a cloud transcription provider and test it.
- As a user, I can enable enhancement and choose different prompts for different scenarios.
- As a user, I can define a profile for a specific app and see when it is active.
- As a user, I can choose a paste method that works in terminals.
- As a user, I can view history, copy past outputs, and re-insert them.
- As a user, if something fails, I can run diagnostics and share a support bundle without exposing secrets.

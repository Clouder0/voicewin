# macOS Insert Main-Thread Crash Fix Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate the macOS crash that happens after recording completes by ensuring every AppKit/HIToolbox interaction in the insert pipeline runs on the main dispatch queue, while preserving current paste behavior and avoiding a 1s UI freeze.

**Architecture:** Keep the async recording/STT pipeline unchanged, but move macOS-only UI side effects into a narrow imperative shell inside `voicewin-platform`. Do **not** wrap the entire paste function in one big main-thread closure, because `paste_text_via_clipboard()` currently sleeps for ~1.1s total; instead, refactor it into short main-thread phases that only touch `NSPasteboard`/HIToolbox and return plain Rust data to the background task.

**Tech Stack:** Rust, Tokio/Tauri, AppKit (`NSPasteboard`), HIToolbox/Carbon (`TIS*`), CoreGraphics (`CGEvent`), cargo test, macOS manual QA, GitHub Actions macOS bundle build.

---

## Progress

- [x] Plan approved
- [x] Task 1: Lock down the main-thread invariant
- [x] Task 2: Refactor pasteboard access into short main-thread phases
- [x] Task 3: Add diagnostics and build provenance
- [ ] Task 4: Verify the fix without regressing UX
- [ ] Task 5: Ship as a focused hotfix and close the feedback loop

## Execution Notes

- Working in the current project as requested; no worktree.
- Local verification in this environment is limited to non-macOS builds/tests unless otherwise noted.

## Verification Notes

- 2026-03-09: `cargo fmt` completed successfully.
- 2026-03-09: `cargo test` completed successfully on the Linux host.
- 2026-03-09: `cargo test` in `voicewin-tauri/src-tauri/` completed successfully.
- 2026-03-09: `cargo check` in `voicewin-tauri/src-tauri/` completed successfully (with pre-existing dead-code warnings in `session_controller.rs`).
- 2026-03-09: `bun run test` in `voicewin-tauri/` completed successfully (`9` files / `19` tests passed).
- 2026-03-09: `bun run build` in `voicewin-tauri/` completed successfully.
- 2026-03-09: `cargo test -p voicewin-platform -- --list` shows `0` host tests because the macOS insert module is behind `cfg(target_os = "macos")`.
- 2026-03-09: `cargo test -p voicewin-platform --target aarch64-apple-darwin --no-run` is blocked in this environment by missing Apple cross-compilation tooling (`ring` fails while invoking Linux `cc` with macOS flags).
- Remaining verification for Task 4 requires a real macOS environment: local `.app` build, manual paste regression matrix, and artifact validation.
- 2026-03-09: release/handoff notes drafted in `docs/releases/2026-03-09-macos-insert-crash-hotfix.md`.
- 2026-03-09: release version updated to `0.1.2`, superseding the initial `0.1.1` cut after review tightened the macOS-only regression-test/release notes while still advancing from the crashing `0.1.0` build.

---

## Recommended Approach

Choose a **phase-based main-thread shell** inside `voicewin-platform/src/macos_insert.rs`.

- **Why this approach:** it fixes the crash at the true boundary (macOS UI/thread affinity), keeps the change local to macOS platform code, and avoids freezing the Tauri main thread during the existing `50ms + 50ms + 1000ms` waits.
- **Do not do:** a Tauri-wide main-thread dispatcher for the whole session pipeline. That leaks macOS concerns into appcore/engine and adds complexity for little value.
- **Do not do:** only patch the currently crashing `TIS*` call sites. The current source already wraps those, but `NSPasteboard` calls still happen off-main in `voicewin-platform/src/macos_insert.rs:423`, `voicewin-platform/src/macos_insert.rs:425`, `voicewin-platform/src/macos_insert.rs:435`, `voicewin-platform/src/macos_insert.rs:439`, and restore/snapshot helpers. Production-grade means locking down the entire AppKit/HIToolbox boundary, not only the frame that crashed once.

### Task 1: Lock Down the Main-Thread Invariant

**Files:**
- Modify: `voicewin-platform/src/macos_insert.rs`
- Test: `voicewin-platform/src/macos_insert.rs`

**Step 1: Write the failing tests**

Add macOS-only unit tests for two invariants:

```rust
#[test]
fn run_on_main_queue_sync_hops_from_worker_thread() {
    let on_main = std::thread::spawn(|| run_on_main_queue_sync(|| unsafe { pthread_main_np() != 0 }))
        .join()
        .unwrap();
    assert!(on_main);
}

#[test]
fn should_restore_pasteboard_only_when_change_count_matches_expected_values() {
    assert!(should_restore_pasteboard(10, 10, 7));
    assert!(should_restore_pasteboard(7, 10, 7));
    assert!(!should_restore_pasteboard(11, 10, 7));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p voicewin-platform run_on_main_queue_sync_hops_from_worker_thread should_restore_pasteboard_only_when_change_count_matches_expected_values`

Expected: fail because `should_restore_pasteboard` does not exist yet.

**Step 3: Write the minimal implementation**

Inside `voicewin-platform/src/macos_insert.rs`:

- Add a small pure helper:

```rust
fn should_restore_pasteboard(current: isize, after_write: isize, original: isize) -> bool {
    current == after_write || current == original
}
```

- Add a debug-only invariant helper for main-thread-only closures:

```rust
fn debug_assert_main_thread(where_: &str) {
    debug_assert!(unsafe { pthread_main_np() } != 0, "{where_} must run on the main thread");
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p voicewin-platform run_on_main_queue_sync_hops_from_worker_thread should_restore_pasteboard_only_when_change_count_matches_expected_values`

Expected: PASS.

**Step 5: Commit**

```bash
git add voicewin-platform/src/macos_insert.rs
git commit -m "test(macOS): lock down insert thread invariants"
```

### Task 2: Refactor Pasteboard Access Into Short Main-Thread Phases

**Files:**
- Modify: `voicewin-platform/src/macos_insert.rs`
- Test: `voicewin-platform/src/macos_insert.rs`

**Step 1: Write the failing test for the new plain-data state object**

Add a unit test that exercises the pure decision logic and a new plain-data state shape:

```rust
#[test]
fn pasteboard_write_state_carries_only_plain_rust_data() {
    let state = PasteboardWriteState {
        original_change: 1,
        after_write_change: 2,
        snapshot: None,
    };
    assert_eq!(state.original_change, 1);
    assert_eq!(state.after_write_change, 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p voicewin-platform pasteboard_write_state_carries_only_plain_rust_data -- --exact`

Expected: fail because `PasteboardWriteState` does not exist yet.

**Step 3: Write the minimal implementation**

Refactor `voicewin-platform/src/macos_insert.rs` into three phases:

1. `capture_and_write_text_on_main(text, trusted) -> PasteboardWriteState`
   - Acquire `NSPasteboard::generalPasteboard()` **inside** `run_on_main_queue_sync`.
   - Read `changeCount`, snapshot existing items, clear contents, write text, read `after_write_change`.
   - Return only plain Rust data (`isize`, `Option<Vec<PasteboardItemSnapshot>>`).
   - Do **not** return or store `NSPasteboard`, `NSArray`, `NSData`, or other AppKit objects outside the closure.

2. Keep waits and event posting off-main:
   - `thread::sleep(Duration::from_millis(50))`
   - `maybe_switch_to_us_qwerty_layout()` (already main-queue wrapped internally)
   - `post_cmd_v()` / `post_enter()`
   - `thread::sleep(Duration::from_millis(1000))`

3. `restore_pasteboard_on_main_if_unchanged(&PasteboardWriteState)`
   - Re-acquire `NSPasteboard::generalPasteboard()` **inside** `run_on_main_queue_sync`.
   - Read current change count.
   - Use `should_restore_pasteboard(...)`.
   - Restore from the plain-data snapshot if still safe.

Suggested shape:

```rust
struct PasteboardWriteState {
    original_change: isize,
    after_write_change: isize,
    snapshot: Option<Vec<PasteboardItemSnapshot>>,
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p voicewin-platform`

Expected: PASS on supported platforms.

**Step 5: Commit**

```bash
git add voicewin-platform/src/macos_insert.rs
git commit -m "fix(macOS): marshal pasteboard access to the main queue"
```

### Task 3: Add Diagnostics So Future Reports Identify the Exact Build and Insert Stage

**Files:**
- Modify: `voicewin-tauri/src-tauri/build.rs`
- Modify: `voicewin-tauri/src-tauri/src/main.rs`
- Modify: `voicewin-platform/src/macos_insert.rs`

**Step 1: Write the failing verification target**

Plan to verify startup logs show both app version and git SHA, and insert logs show phase markers:

```text
VoiceWin startup: version=0.1.0 git_sha=abcdef1
macOS insert phase=start
macOS insert phase=pasteboard_written
macOS insert phase=restore_skipped_or_restored
```

**Step 2: Run the existing app and confirm the metadata is missing**

Run: `cargo run -p voicewin-tauri`

Expected: startup logs do not yet include an explicit git SHA and insert phases are not consistently visible.

**Step 3: Write the minimal implementation**

- In `voicewin-tauri/src-tauri/build.rs`, export a best-effort git SHA env var during build.
- In `voicewin-tauri/src-tauri/src/main.rs`, log `CARGO_PKG_VERSION` + that SHA at startup.
- In `voicewin-platform/src/macos_insert.rs`, add compact stage logs around:
  - start
  - pasteboard write complete
  - layout switch attempted
  - paste event posted
  - restore performed / skipped

Keep logs short and structured so field crash reports can be tied to a specific build and phase.

**Step 4: Run and verify**

Run: `cargo run -p voicewin-tauri`

Expected: startup log includes git SHA; triggering a macOS paste session shows phase logs.

**Step 5: Commit**

```bash
git add voicewin-tauri/src-tauri/build.rs voicewin-tauri/src-tauri/src/main.rs voicewin-platform/src/macos_insert.rs
git commit -m "chore(macOS): add insert diagnostics and build provenance"
```

### Task 4: Verify the Fix Without Regressing UX

**Files:**
- Modify: `voicewin-platform/src/macos_insert.rs` (only if small follow-up adjustments are needed)
- Build/Test: `.github/workflows/voicewin-macos-unsigned.yml`

**Step 1: Run automated tests**

Run: `cargo test`

Expected: PASS.

**Step 2: Run frontend tests**

Run: `cd voicewin-tauri && bun run test`

Expected: PASS.

**Step 3: Build the macOS app bundle locally**

Run: `cd voicewin-tauri && cargo tauri build --target aarch64-apple-darwin`

Expected: build succeeds and produces a `.app` bundle.

**Step 4: Execute the macOS manual regression matrix**

Validate these scenarios on macOS 15.x arm64:

1. TextEdit + default ABC layout + regular paste
2. TextEdit + non-US layout + regular paste
3. TextEdit + non-US layout + `PasteAndEnter`
4. Target app changed clipboard during the 1s restore delay
5. Accessibility disabled (expect graceful error, no crash)
6. Rapid repeated record/stop cycles
7. Overlay/hotkey responsiveness during insert (confirm no visible 1s freeze)

Capture logs for each run.

**Step 5: Commit any small validation-only adjustments**

```bash
git add voicewin-platform/src/macos_insert.rs
git commit -m "fix(macOS): polish insert regression handling"
```

### Task 5: Ship as a Focused Hotfix and Close the Feedback Loop

**Files:**
- Modify: release notes / changelog location used by the team (if any)
- No code changes unless a release-only adjustment is required

**Step 1: Cut a focused hotfix release**

Keep scope narrow:

- main-thread-safe macOS insert path
- build provenance in logs
- no unrelated refactors

**Step 2: Verify the shipped artifact really contains the fix**

Before distributing the `.app`:

- launch it once
- confirm startup log shows the expected git SHA
- trigger one successful insert and confirm the new phase logs appear

**Step 3: Publish with a clear regression note**

Release note text should say the build fixes a macOS crash that could happen immediately after recording completed while VoiceWin pasted text back into the target app.

**Step 4: Monitor field feedback**

For the first macOS users on the hotfix, collect:

- app version + git SHA
- whether Accessibility was enabled
- active keyboard layout
- target app name
- whether restore was logged as performed or skipped

**Step 5: Commit release metadata if applicable**

```bash
git add <release-note-files>
git commit -m "docs(release): note macOS insert crash hotfix"
```

## Notes for Implementation

- The current crash evidence points to thread-affinity violations after recording completes, not to STT/audio logic.
- `voicewin-tauri/src-tauri/src/session_controller.rs:1157` and `voicewin-tauri/src-tauri/src/session_controller.rs:1163` explicitly run the session pipeline in a background task; that is fine and should stay.
- The invariant to enforce is simple: **no AppKit/HIToolbox object or API call leaves the main-queue shell**.
- Preserve current behavior unless verification proves a change is needed: clipboard snapshot/restore, layout switching, `PasteAndEnter`, and accessibility messaging.
- If a follow-up is needed after the hotfix, the next step would be a dedicated macOS insertion actor/dispatcher. Do not build that first.

# OpenMouseGesture Project Reference

## Purpose

OpenMouseGesture is a Windows mouse-gesture utility implemented with Tauri 2, React, TypeScript, and Rust.

This file is the single canonical project document for the repository. It describes the current implementation and current operational rules. It is not a chronological task log.

Historical work logs, superseded plans, temporary reports, and obsolete verification notes must not be kept as competing Markdown documents in this repository. Historical information belongs in the external knowledge-management system. When implementation changes, update this file and the code; do not create a new status Markdown file unless the user explicitly requests one.

## Current Status

- Active source: `source-v1.0.1/7-rate-OpenMouseGesture-b8f5357/`
- Stable Windows installer handoff: `dist/windows/OpenMouseGesture-Setup-x64.exe`
- Current application baseline: commit `0458aeb`
- The current mouse-gesture repair cycle is complete for practical use.
- The user will continue validating the application during normal operation and reopen work only if another material defect is found.

## Verified User Behavior

The following behavior was confirmed on the user's Windows machine:

- Gestures work with ordinary keyboard keys and other bindings that successfully register and trigger on that machine.
- Escape cancels an active or release-pending gesture without dispatching an action.
- The 120 ms confirmed-release handling substantially reduced premature single-segment dispatch during multi-direction gestures.
- A single left click on the tray icon toggles gestures enabled/disabled.
- Repeated tray left clicks alternate enabled -> disabled -> enabled.
- Tray left click does not open Settings.
- The tray right-click menu contains only `設定を開く` and `終了`.

## Known Unresolved Input Limitations

### Caps Lock

Caps Lock is not considered supported in the current product state.

On the user's machine:

- it cannot be reliably captured in Settings;
- an existing saved Caps Lock binding does not start a gesture;
- neither a physical Caps Lock press nor a mouse-vendor/remapper mapping to Caps Lock works end-to-end.

Several implementation attempts and automated tests did not reproduce the successful behavior claimed by code-level tests. Do not describe Caps Lock as working without a new physical verification on the target machine.

### Kana and Japanese IME keys

Kana and other Japanese IME-specific keys are not considered supported in the current product state.

They do not reliably register or trigger on the user's Japanese Windows environment. Do not claim support based only on frontend key names, static VK mappings, or automated tests.

### Shift, Alt, and other special keys

Special-key behavior has varied across builds. Only keys whose capture and trigger behavior are physically verified on the target machine should be used for production operation. Ordinary keys are the recommended fallback.

## Trigger Model

- Trigger slots: A, B, C.
- Gesture actions resolve by `trigger_slot + gesture_name`.
- Mouse bindings use `mouse:right`, `mouse:middle`, `mouse:x1`, and `mouse:x2`.
- Keyboard bindings use `key:<Code>` or `key:<Modifier+...+Code>`.
- Left mouse button is prohibited as a gesture-start trigger.
- Duplicate trigger assignments are resolved in A -> B -> C priority order; the first matching slot is effective.

## Gesture Stability and Cancellation

- Gesture release uses a 120 ms confirmation window to tolerate transient key-up/key-down interruptions.
- Escape is the emergency cancellation path for active and release-pending sessions.
- Cancellation must never dispatch an action.
- Disable, shutdown, settings changes, hook restart, and unrecoverable session errors must use the centralized idempotent teardown path.
- No change may reintroduce a permanently stuck red trajectory or active session.

## Tray and Window Behavior

- Tray left click toggles gesture enabled state.
- Tray right-click menu contains only Settings and Exit.
- Disabling gestures cancels any active or pending gesture without dispatch.
- Tray icon and tooltip should reflect enabled/disabled state immediately.
- Closing the main window hides it instead of terminating the resident process.
- Tray initialization failure must not prevent hook installation; the main window is the fallback.

## Preserved Product Features

- Trigger A/B/C.
- Per-trigger trajectory colors.
- Per-slot gesture actions.
- Per-slot wheel-up and wheel-down actions.
- Grouped action list and action group reassignment.
- Keystroke, command, URL, window-operation, and literal Unicode text actions.
- Maximize/restore toggle.
- Settings export/import.
- Backup before destructive config reset or sanitation.
- Per-user Windows autostart.
- Single-instance behavior.
- Right-click short-click passthrough.
- Action-label overlay remains intentionally disabled for stability.

## Settings and Persistence

Release settings are stored under:

`%AppData%\GestureHotkeyApp\`

Primary persisted files:

- `config.json`
- `gestures.json`

Existing valid custom actions must not be replaced by bundled defaults during normal load or normalization. Destructive reset and sanitation paths must create a backup first.

## Build, Test, and Distribution

From the active source directory:

```powershell
cd C:\GitHub\open-mouse-gesture\source-v1.0.1\7-rate-OpenMouseGesture-b8f5357
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npm run tauri build
```

From the repository root:

```powershell
cd C:\GitHub\open-mouse-gesture
npm run dist:windows
npm run test:dist
git diff --check
```

The distribution export copies the current release outputs to:

- `dist/windows/OpenMouseGesture-x64.exe`
- `dist/windows/OpenMouseGesture-Setup-x64.exe`
- `dist/windows/SHA256SUMS.txt`
- `dist/windows/build-info.json`

Automated tests are necessary but do not override contradictory physical results for low-level keyboard, remapper, tray, or installer behavior.

## Repository Rules

- Treat `PROJECT.md` as the single canonical Markdown document for current project state and operating rules.
- Treat `source-v1.0.1/7-rate-OpenMouseGesture-b8f5357/` as the active source of truth for implementation.
- Do not create task logs, progress logs, completion reports, handoff notes, or dated status Markdown files inside the repository unless explicitly requested.
- Store historical logs and superseded decisions in the external knowledge-management system, not in competing repository documents.
- Update this file when a current product fact, limitation, build flow, or operational rule changes.
- Preserve unrelated workspace changes.
- Do not rewrite history or force-push for routine maintenance.
- Keep root-level distribution tooling synchronized with the actual Tauri bundle layout.
- Do not rely on old planning documents when they conflict with current code or current physical verification.

## Reopening Special-Key Investigation

If Caps Lock, Kana, or another special-key issue is reopened, diagnose it on the target machine while recording both layers at the same time:

- WebView: `event.key`, `event.code`, `location`, `repeat`.
- Windows hook: VK, scan code, extended flag, injected flag, and `dwExtraInfo`.
- Physical keyboard input versus mouse-remapper-generated input.
- Persisted trigger identity versus runtime matching identity.

Do not mark the issue solved until capture, persistence, reload, physical triggering, release, and suppression behavior are all verified on the user's machine.
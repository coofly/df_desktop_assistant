# Settings Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a tray-launched settings window that controls automatic refresh frequency and Windows startup launch.

**Architecture:** Rust/Tauri owns tray entries, settings persistence, validation, and Windows startup registration. Svelte adds a `/settings` route for the compact settings form and teaches the main route to schedule automatic refreshes from persisted settings.

**Tech Stack:** Rust, Tauri 2, SvelteKit static SPA, Svelte 5 runes, Windows PowerShell validation commands.

---

### Task 1: Rust Settings Model and Persistence

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] Write failing unit tests for default settings, interval clamping, and settings file round-trip.
- [ ] Run `cd src-tauri; cargo test settings_defaults_to_five_minutes settings_clamps_refresh_interval settings_file_round_trips`.
- [ ] Add `AssistantSettings`, validation helpers, settings file read/write helpers, and tests.
- [ ] Re-run the same tests and confirm they pass.

### Task 2: Startup Registration and Commands

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] Write failing unit tests for Windows `reg.exe` startup add/delete command construction.
- [ ] Run `cd src-tauri; cargo test startup_registration_builds_expected_commands`.
- [ ] Add startup command construction and `apply_launch_on_startup`.
- [ ] Add Tauri commands `get_settings` and `save_settings`.
- [ ] Re-run the startup tests and existing Rust tests.

### Task 3: Tray Settings Window

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`

- [ ] Add a tray menu item with id `open_settings`.
- [ ] Add `open_settings_window` using Tauri's webview window builder and route `/settings`.
- [ ] Include the `settings` window in the capability file.
- [ ] Register `get_settings` and `save_settings` in `invoke_handler`.
- [ ] Run `cd src-tauri; cargo check`.

### Task 4: Svelte Settings Route

**Files:**
- Create: `src/routes/settings/+page.svelte`

- [ ] Build a compact settings panel with refresh interval slider/number input and startup toggle.
- [ ] Load settings with `get_settings`.
- [ ] Save through `save_settings` and show success/error status.
- [ ] Keep browser preview fallback values for non-Tauri preview.

### Task 5: Main Auto Refresh

**Files:**
- Modify: `src/routes/+page.svelte`

- [ ] Add the `AssistantSettings` type.
- [ ] Load settings on startup.
- [ ] Schedule `loadRecommendations` with `window.setInterval` using `refreshIntervalMinutes`.
- [ ] Clear and recreate the interval if settings are loaded again.
- [ ] Keep manual refresh behavior unchanged.

### Task 6: Verification

**Files:**
- Verify only

- [ ] Run `cd src-tauri; cargo test`.
- [ ] Run `cd src-tauri; cargo check`.
- [ ] Run `npm run check`, using the local dependency entry if the npm shim is blocked.
- [ ] Report any environment-specific failures with exact command and failure reason.

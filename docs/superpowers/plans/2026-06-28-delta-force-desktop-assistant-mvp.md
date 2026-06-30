# Delta Force Desktop Assistant MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the default Tauri + SvelteKit template into a usable Delta Force crafting recommendation desktop assistant with tray controls and lock state.

**Architecture:** Rust owns Tauri window lifecycle, tray menu, lock state, and mock recommendation data. Svelte owns compact presentation, sorting, refresh status, and loading/error states through typed command responses. Documentation records the live data boundary for future kkrb.net integration.

**Tech Stack:** Rust, Tauri 2, Svelte 5, SvelteKit, TypeScript, Vite.

---

## File Structure

- Modify `src-tauri/src/lib.rs`: replace template command with recommendation commands, add app state, tray menu, close-to-tray behavior, and lock/unlock window movement behavior.
- Modify `src-tauri/Cargo.toml`: keep existing dependencies and use Tauri APIs already available through `tauri = "2"`.
- Modify `src-tauri/tauri.conf.json`: set product/title/window size for a compact desktop assistant.
- Modify `src/routes/+page.svelte`: replace template UI with assistant panel and typed command calls.
- Modify `README.md`: document purpose, setup, commands, architecture, MVP behavior, and roadmap.
- Create `AGENTS.md`: document agent working rules and project-specific constraints.
- Modify `.gitignore`: ignore `.superpowers/` local brainstorm artifacts.

### Task 1: Rust Tauri App State, Commands, Tray, and Window Behavior

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Replace template command with typed assistant data**

Implement `CraftRecommendation`, `AssistantSnapshot`, `AssistantState`, `get_recommendations`, `get_lock_state`, and `set_lock_state`.

- [ ] **Step 2: Add tray menu**

Create tray menu items: show/hide, lock, unlock, quit. Wire each menu id to window/app behavior.

- [ ] **Step 3: Hide to tray on close**

Intercept main window close and hide the window instead of exiting.

- [ ] **Step 4: Apply lock behavior**

Use window resizable/movable controls where available so lock prevents repositioning but does not disable frontend interaction.

### Task 2: Svelte Assistant UI

**Files:**
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Define TypeScript types matching Rust response**

Define `CraftRecommendation`, `AssistantSnapshot`, `SortMode`, and component state.

- [ ] **Step 2: Load recommendations from Tauri**

Call `invoke("get_recommendations")`; use fallback mock data only for browser preview if Tauri invoke is unavailable.

- [ ] **Step 3: Render compact recommendation panel**

Render header, lock chip, sort toggle, refresh button, updated time, and horizontal recommendation cards.

- [ ] **Step 4: Add refresh and sorting behavior**

Refresh updates state from command and timestamp. Sort modes switch between highest profit and station order.

### Task 3: Documentation

**Files:**
- Modify: `README.md`
- Create: `AGENTS.md`
- Modify: `.gitignore`

- [ ] **Step 1: Rewrite README**

Document project purpose, MVP features, tray controls, development commands, data-source boundary, and roadmap.

- [ ] **Step 2: Add AGENTS.md**

Document PowerShell SSH quoting instruction, Rust/Svelte boundaries, tray requirements, verification commands, and ignored artifacts.

- [ ] **Step 3: Ignore brainstorm artifacts**

Add `.superpowers/` to `.gitignore`.

### Task 4: Verification

**Files:**
- Validate all modified files.

- [ ] **Step 1: Run frontend check**

Run `npm run check`. Expected: Svelte/TypeScript check passes.

- [ ] **Step 2: Run Rust check**

Run `cargo check` in `src-tauri`. Expected: Rust/Tauri code compiles.

- [ ] **Step 3: Fix any compile/type issues**

Adjust code until both checks pass or report a dependency/tooling blocker clearly.

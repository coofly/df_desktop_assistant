# Kkrb Live Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace mock crafting recommendations with kkrb live JSON data while preserving the existing Tauri command contract.

**Architecture:** Rust owns the data source. A new `kkrb` module performs the same HTTP sequence as the web page (`getMenu` -> `checkUAStatus` -> `getOVData`) using `reqwest` 0.13 with the `rustls` feature, normalizes `sphData` into `CraftRecommendation`, and falls back to cached/mock data on failure.

**Tech Stack:** Rust 2021, Tauri 2 commands, `reqwest` with `blocking`, `cookies`, `form`, `json`, and `rustls` features, `serde`/`serde_json`, Rust unit tests.

---

### Task 1: Parser And Normalization

**Files:**
- Create: `src-tauri/src/kkrb.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] Write tests for normalizing `sphData` JSON into four `CraftRecommendation` values.
- [ ] Run `cargo test kkrb` and verify the test fails because the module does not exist.
- [ ] Add the `kkrb` module with response structs and `normalize_swat_recommendations`.
- [ ] Re-run `cargo test kkrb` and verify it passes.

### Task 2: HTTP Client Flow

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/kkrb.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] Add explicit `reqwest = { version = "0.13", default-features = false, features = ["blocking", "cookies", "form", "json", "rustls"] }`.
- [ ] Add `fetch_live_recommendations` that preserves cookies with `cookie_store(true)`, sends `X-Requested-With: XMLHttpRequest`, and calls `getMenu`, `checkUAStatus`, `getOVData`.
- [ ] Add tests for mapping API fields and fallback behavior without performing network calls.
- [ ] Run `cargo test`.

### Task 3: State Cache And Command Integration

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/routes/+page.svelte`

- [ ] Change `AssistantState` to cache the last successful recommendations and status text.
- [ ] Keep `get_recommendations` synchronous for the existing Tauri command shape and return live data when available, cached data on transient failure, mock data as final fallback.
- [ ] Keep the existing `AssistantSnapshot` and `CraftRecommendation` camelCase fields stable.
- [ ] Update frontend preview text only if needed to reflect live/cached status.
- [ ] Run `npm run check` and `cargo test`.

### Task 4: Verification

**Files:**
- No additional files expected.

- [ ] Run `cargo check` inside `src-tauri`.
- [ ] Run `npm run check`.
- [ ] Optionally run `npm run tauri dev` to manually confirm cards show live kkrb values.

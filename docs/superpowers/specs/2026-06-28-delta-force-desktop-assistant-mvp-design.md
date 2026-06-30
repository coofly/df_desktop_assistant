# Delta Force Desktop Assistant MVP Design

## Goal

Initialize the existing Rust + Tauri + Svelte project as a desktop helper for Delta Force crafting recommendations. The MVP should replace the default template with a usable recommendation panel, include tray-based lifecycle controls, and document how future live data from `https://www.kkrb.net/?viewpage=view%2Foverview` should be integrated.

## Product Scope

The first screen is the actual assistant UI, not a landing page. It shows recommendation cards for Special Ops crafting outputs: station name, recommended product, current profit, ideal sell price, suggested sell time, and a freshness indicator.

The app starts with mock structured data. Live website integration is out of scope for this initialization because the target page likely requires rendered-page or internal API analysis. The code and docs must leave a clean boundary so a future data-source module can fetch, cache, and parse live data while Svelte consumes a stable typed recommendation list.

## Window and Tray Behavior

The app runs as a compact desktop window suitable for staying near the edge of the screen. The initial window is wide and short, matching the provided screenshot direction.

The app must create a system tray icon at startup. The tray menu controls:

- Show or hide the main window.
- Lock the window position.
- Unlock the window position.
- Exit the program.

Closing the main window should hide it to the tray instead of exiting. Exiting should be done from the tray menu. Locking means the window position should stay fixed and the user should not accidentally drag or reposition the assistant. Locking must not disable interaction with the UI; recommendation cards, refresh controls, and sort controls remain usable.

## Frontend Behavior

The Svelte UI should show:

- Header with app title, lock status, profit sort/mode switch, and last updated status.
- Horizontal recommendation cards based on mock data.
- Card fields for station, profit, ideal sell price, sell time, item name, item category, and a stable colored item thumbnail area.
- A refresh action that updates the displayed timestamp and keeps the mock data stable.

The visual style should be quiet and utility-focused: compact spacing, readable numbers, clear cards, and no marketing hero content.

## Rust/Tauri Boundary

Rust exposes Tauri commands for:

- Returning current recommendations.
- Returning or changing lock state.
- Handling tray menu commands.

The MVP can keep the recommendation source in Rust as mock data or return a static structure from a command. The frontend should call the command through `@tauri-apps/api/core` and handle loading/error states.

Future live data should be implemented behind the same command contract:

1. Fetch target site data or discovered internal API.
2. Normalize fields into `CraftRecommendation`.
3. Cache the latest successful result.
4. Return cached data on transient failures with an error/status field.

## Documentation

`README.md` should describe the app, tech stack, dev commands, current MVP status, data-source boundary, and roadmap.

`AGENTS.md` should document working rules for future agents, including:

- Use Windows PowerShell carefully when invoking remote SSH scripts; wrap remote commands in single quotes and do not use Bash-style `\"` escaping for PowerShell strings.
- Keep Rust/Tauri window and tray lifecycle logic in Rust.
- Keep Svelte presentation consuming typed data structures rather than scraping directly.
- Do not commit transient brainstorming files or generated build output.

## Verification

Run frontend type checking and Rust compilation checks where dependencies allow:

- `npm run check`
- `cargo check` inside `src-tauri`

If dependencies are missing, install or sync them with the project's package manager before verification.

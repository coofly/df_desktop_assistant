# Settings Window Design

## Goal

Add a tray-launched settings window for the desktop assistant. The settings window controls automatic recommendation refresh frequency and Windows startup launch.

## User Experience

The tray menu gains a `设置` item. Selecting it opens a compact independent settings window instead of changing the main recommendation panel. The main panel remains the first useful assistant surface and does not become a marketing or setup page.

The settings window contains two controls:

- `刷新频率`: numeric/range control from 1 to 60 minutes, default 5 minutes.
- `开机自启动`: toggle that writes or removes a Windows startup entry.

The settings copy uses player-facing terms and stays short: `刷新频率`, `分钟`, `开机自启动`, `保存设置`.

## Architecture

Rust owns settings persistence, startup registration, and tray/window behavior. Svelte owns the settings form and automatic refresh timer.

`AssistantSettings` is a stable command payload:

```json
{
  "refreshIntervalMinutes": 5,
  "launchOnStartup": false
}
```

Rust commands:

- `get_settings`: returns persisted settings, falling back to defaults.
- `save_settings`: validates and persists settings, clamps refresh interval to 1-60 minutes, and applies startup registration.

The frontend adds `/settings` as a static SPA route. The main route reads settings on startup and resets its refresh timer whenever settings change. A browser preview fallback keeps the current 5-minute default.

## Data Flow

1. App starts and creates tray.
2. Main window loads recommendations once.
3. Main route calls `get_settings` and schedules automatic `get_recommendations` calls based on `refreshIntervalMinutes`.
4. Tray `设置` opens or focuses the settings window.
5. Settings window calls `get_settings`, lets the user edit values, then calls `save_settings`.
6. Rust persists the JSON file and updates Windows startup registration.

## Error Handling

Invalid refresh values are clamped to the supported range. If startup registration fails, `save_settings` returns an error and the settings window shows a short status message. If settings cannot be loaded in browser preview, the UI uses the default 5-minute refresh interval.

## Testing

Rust unit tests cover default settings, refresh interval clamping, settings file round-trip, and Windows startup command construction. Frontend verification uses `npm run check`; Rust verification uses `cargo test` and `cargo check`.

## Notes

The current workspace does not expose Git metadata, so this design cannot be committed from this session.

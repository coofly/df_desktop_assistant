<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type AssistantSettings = {
    refreshIntervalMinutes: number;
    launchOnStartup: boolean;
    mainWindowOpacityPercent: number;
  };

  const defaultSettings: AssistantSettings = {
    refreshIntervalMinutes: 5,
    launchOnStartup: false,
    mainWindowOpacityPercent: 5
  };

  let settings = $state<AssistantSettings>(defaultSettings);
  let saving = $state(false);
  let error = $state("");
  let logFilePath = $state("");

  function logFileName() {
    if (!logFilePath) {
      return "仅在桌面应用内可用";
    }

    return logFilePath.split(/[\\/]/).pop() || logFilePath;
  }

  function clampInterval(value: number) {
    if (Number.isNaN(value)) {
      return defaultSettings.refreshIntervalMinutes;
    }

    return Math.min(60, Math.max(1, Math.round(value)));
  }

  function clampOpacity(value: number) {
    if (Number.isNaN(value)) {
      return defaultSettings.mainWindowOpacityPercent;
    }

    return Math.min(100, Math.max(5, Math.round(value)));
  }

  async function writeLog(message: string) {
    try {
      await invoke("write_frontend_log", { source: "设置页", message });
    } catch {
      // Browser preview cannot call the Tauri log command.
    }
  }

  async function loadSettings() {
    try {
      settings = await invoke<AssistantSettings>("get_settings");
    } catch {
      settings = defaultSettings;
      writeLog("设置读取失败，使用预览默认值");
    }
  }

  async function saveSettings() {
    saving = true;
    error = "";
    settings = {
      ...settings,
      refreshIntervalMinutes: clampInterval(settings.refreshIntervalMinutes),
      mainWindowOpacityPercent: clampOpacity(settings.mainWindowOpacityPercent)
    };

    try {
      settings = await invoke<AssistantSettings>("save_settings", {
        refreshIntervalMinutes: settings.refreshIntervalMinutes,
        launchOnStartup: settings.launchOnStartup,
        mainWindowOpacityPercent: settings.mainWindowOpacityPercent
      });
      await closeSettings();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause || "保存设置失败");
      writeLog(`设置保存失败：${error}`);
    } finally {
      saving = false;
    }
  }

  async function loadLogFilePath() {
    try {
      logFilePath = await invoke<string>("get_log_file_path");
    } catch {
      logFilePath = "";
    }
  }

  async function beginSettingsDrag(event: PointerEvent) {
    if (event.button !== 0) {
      return;
    }

    try {
      await invoke("start_settings_window_drag");
    } catch {
      // Browser preview cannot call the Tauri window command.
    }
  }

  async function closeSettings() {
    try {
      await invoke("close_settings_window");
    } catch {
      // Browser preview cannot call the Tauri window command.
    }
  }

  async function openLogFolder() {
    try {
      await invoke("open_log_folder");
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause || "打开日志文件夹失败");
      writeLog(`打开日志文件夹失败：${error}`);
    }
  }

  $effect(() => {
    loadSettings();
    loadLogFilePath();
  });
</script>

<svelte:head>
  <title>设置</title>
</svelte:head>

<main class="settings-shell">
  <header
    class="top-bar"
    role="button"
    tabindex="0"
    aria-label="拖动设置窗口"
    onpointerdown={beginSettingsDrag}
  >
    <div class="title-block">
      <span class="rail"></span>
      <div>
        <h1>设置</h1>
      </div>
    </div>

    <div
      class="window-controls"
      role="group"
      aria-label="设置窗口控制"
      onpointerdown={(event) => event.stopPropagation()}
    >
      <button class="icon-button" type="button" title="关闭设置" aria-label="关闭设置" onclick={closeSettings}>
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M6 6l12 12" />
          <path d="M18 6L6 18" />
        </svg>
      </button>
    </div>
  </header>

  <section class="settings-list" aria-label="设置项">
    <article class="setting-row range-row">
      <div class="copy">
        <label for="refresh-interval">刷新频率</label>
        <p>每 {settings.refreshIntervalMinutes} 分钟刷新</p>
      </div>
      <div class="interval-control">
        <input
          id="refresh-interval"
          type="range"
          min="1"
          max="60"
          bind:value={settings.refreshIntervalMinutes}
        />
        <span class="unit-label">分钟</span>
        <input
          aria-label="刷新频率分钟数"
          class="number-input"
          type="number"
          min="1"
          max="60"
          bind:value={settings.refreshIntervalMinutes}
          onblur={() =>
            (settings = {
              ...settings,
              refreshIntervalMinutes: clampInterval(settings.refreshIntervalMinutes)
            })}
        />
      </div>
    </article>

    <article class="setting-row">
      <div class="copy">
        <label for="launch-on-startup">开机自启动</label>
        <p>登录 Windows 后自动打开</p>
      </div>
      <label class="switch" for="launch-on-startup" aria-label="开机自启动">
        <input id="launch-on-startup" type="checkbox" bind:checked={settings.launchOnStartup} />
        <span></span>
      </label>
    </article>

    <article class="setting-row range-row">
      <div class="copy">
        <label for="panel-opacity">面板透明度</label>
        <p>当前 {settings.mainWindowOpacityPercent}%</p>
      </div>
      <div class="interval-control">
        <input
          id="panel-opacity"
          type="range"
          min="5"
          max="100"
          bind:value={settings.mainWindowOpacityPercent}
        />
        <span class="unit-label">%</span>
        <input
          aria-label="面板透明度百分比"
          class="number-input"
          type="number"
          min="5"
          max="100"
          bind:value={settings.mainWindowOpacityPercent}
          onblur={() =>
            (settings = {
              ...settings,
              mainWindowOpacityPercent: clampOpacity(settings.mainWindowOpacityPercent)
            })}
        />
      </div>
    </article>

    <article class="setting-row log-row">
      <div class="copy">
        <label for="log-path">日志文件</label>
        <p id="log-path" title={logFilePath}>{logFileName()}</p>
      </div>
      <button class="secondary-button" type="button" onclick={openLogFolder}>打开</button>
    </article>
  </section>

  <footer>
    {#if error}
      <span class="error-message">{error}</span>
    {/if}
    <button class="save-button" type="button" onclick={saveSettings} disabled={saving}>
      {saving ? "保存中" : "保存设置"}
    </button>
  </footer>
</main>

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    overflow: hidden;
    color: #ffffff;
    background: transparent;
    font-family:
      "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI", system-ui, sans-serif;
  }

  :global(button),
  :global(input) {
    font: inherit;
  }

  .settings-shell {
    display: grid;
    grid-template-rows: auto 1fr auto;
    gap: 8px;
    width: 100vw;
    height: 100vh;
    padding: 10px;
    border: 1px solid #303336;
    border-radius: 0;
    background: rgb(239 240 238 / 10%);
  }

  .top-bar {
    display: flex;
    min-height: 38px;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    border: 1px solid #50545a;
    padding: 4px 6px 4px 8px;
    cursor: grab;
    user-select: none;
    background: rgb(0 0 0 / 30%);
  }

  .title-block {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 10px;
  }

  .rail {
    width: 8px;
    height: 24px;
    border: 1px solid #303336;
    background: rgb(23 25 26 / 12%);
  }

  h1 {
    margin: 0;
    color: #ffffff;
    font-size: 15px;
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: 0;
  }

  .window-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .icon-button {
    display: grid;
    width: 30px;
    height: 30px;
    min-width: 30px;
    place-items: center;
    border: 0;
    border-radius: 0;
    padding: 0;
    color: #d7dbe0;
    background: transparent;
    cursor: pointer;
  }

  .icon-button svg {
    display: block;
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: square;
    stroke-linejoin: miter;
    stroke-width: 1.8;
  }

  .icon-button:hover {
    color: #ffffff;
    background: rgb(255 255 255 / 12%);
  }

  .settings-list {
    display: grid;
    gap: 8px;
    min-height: 0;
  }

  .setting-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 68px;
    min-height: 58px;
    align-items: center;
    gap: 12px;
    overflow: hidden;
    border: 1px solid #50545a;
    padding: 10px 12px;
    color: #ffffff;
    background: rgb(0 0 0 / 30%);
  }

  .range-row {
    grid-template-columns: minmax(0, 1fr) 180px;
  }

  .copy {
    min-width: 0;
  }

  label {
    display: block;
    margin: 0 0 5px;
    color: #ffffff;
    font-size: 13px;
    font-weight: 800;
    line-height: 1.15;
  }

  p {
    margin: 0;
    color: #cfd3d7;
    font-size: 12px;
    line-height: 1.35;
    white-space: nowrap;
  }

  .interval-control {
    display: grid;
    grid-template-columns: 84px 28px 68px;
    align-items: center;
    justify-content: end;
    gap: 7px;
    color: #cfd3d7;
    font-size: 12px;
  }

  input[type="range"] {
    width: 84px;
    accent-color: #1f3f73;
  }

  .unit-label {
    justify-self: end;
    min-width: 28px;
    text-align: right;
  }

  .number-input {
    width: 68px;
    height: 32px;
    border: 1px solid #50545a;
    border-radius: 0;
    color: #ffffff;
    padding: 0 2px 0 6px;
    text-align: center;
    background: rgb(12 18 16 / 22%);
    appearance: textfield;
  }

  .number-input::-webkit-inner-spin-button,
  .number-input::-webkit-outer-spin-button {
    margin: 0;
    appearance: none;
  }

  .number-input:focus-visible {
    border-color: #1f3f73;
    outline: none;
  }

  .save-button:focus-visible,
  .secondary-button:focus-visible,
  .icon-button:focus-visible,
  .switch input:focus-visible + span {
    outline: 1px solid #ffffff;
    outline-offset: 2px;
  }

  .switch {
    position: relative;
    display: block;
    justify-self: end;
    width: 68px;
    height: 32px;
    margin: 0;
    cursor: pointer;
  }

  .switch input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .switch span {
    position: absolute;
    inset: 0;
    border: 1px solid #50545a;
    background: rgb(12 18 16 / 22%);
  }

  .switch span::after {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 18px;
    height: 18px;
    border: 1px solid #303336;
    background: #cfd3d7;
    content: "";
    transition: transform 120ms ease, background 120ms ease;
  }

  .switch input:checked + span {
    background: #1f3f73;
  }

  .switch input:checked + span::after {
    background: #ffffff;
    transform: translateX(36px);
  }

  .log-row {
    min-height: 54px;
  }

  .log-row p {
    max-width: 210px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 0;
  }

  .error-message {
    min-width: 0;
    overflow: hidden;
    color: #ffd2be;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .secondary-button,
  .save-button {
    height: 32px;
    border: 1px solid #50545a;
    border-radius: 0;
    padding: 0 14px;
    color: #ffffff;
    cursor: pointer;
  }

  .secondary-button {
    justify-self: end;
    width: 68px;
    min-width: 68px;
    padding: 0;
    background: rgb(12 18 16 / 22%);
  }

  .secondary-button:hover {
    background: rgb(255 255 255 / 12%);
  }

  footer:empty {
    justify-content: flex-end;
  }

  .save-button {
    min-width: 96px;
    margin-left: auto;
    background: #1f3f73;
  }

  .save-button:hover {
    background: #2b548f;
  }

  .save-button:disabled {
    cursor: wait;
    opacity: 0.62;
  }
</style>

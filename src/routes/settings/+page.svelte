<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type AssistantSettings = {
    refreshIntervalMinutes: number;
    launchOnStartup: boolean;
  };

  const defaultSettings: AssistantSettings = {
    refreshIntervalMinutes: 5,
    launchOnStartup: false
  };

  let settings = $state<AssistantSettings>(defaultSettings);
  let saving = $state(false);
  let status = $state("未保存");
  let error = $state("");
  let logFilePath = $state("");

  function clampInterval(value: number) {
    if (Number.isNaN(value)) {
      return defaultSettings.refreshIntervalMinutes;
    }

    return Math.min(60, Math.max(1, Math.round(value)));
  }

  async function writeLog(message: string) {
    try {
      await invoke("write_frontend_log", { source: "设置页", message });
    } catch {
      // Browser preview cannot call the Tauri log command.
    }
  }

  async function loadSettings() {
    writeLog("开始读取设置");
    try {
      settings = await invoke<AssistantSettings>("get_settings");
      status = "已载入";
      writeLog(`设置读取成功，刷新间隔：${settings.refreshIntervalMinutes} 分钟，开机自启动：${settings.launchOnStartup ? "开启" : "关闭"}`);
    } catch {
      settings = defaultSettings;
      status = "预览默认值";
      writeLog("设置读取失败，使用预览默认值");
    }
  }

  async function saveSettings() {
    saving = true;
    error = "";
    writeLog("开始保存设置");
    settings = {
      ...settings,
      refreshIntervalMinutes: clampInterval(settings.refreshIntervalMinutes)
    };

    try {
      settings = await invoke<AssistantSettings>("save_settings", {
        refreshIntervalMinutes: settings.refreshIntervalMinutes,
        launchOnStartup: settings.launchOnStartup
      });
      status = "已保存";
      writeLog(`设置保存成功，刷新间隔：${settings.refreshIntervalMinutes} 分钟，开机自启动：${settings.launchOnStartup ? "开启" : "关闭"}`);
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
      writeLog(`日志文件路径读取成功：${logFilePath}`);
    } catch {
      logFilePath = "";
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
  <header>
    <div>
      <p class="kicker">特勤处助手</p>
      <h1>设置</h1>
    </div>
    <span class="status" class:error={error}>{error || status}</span>
  </header>

  <section class="setting-row">
    <div>
      <label for="refresh-interval">刷新频率</label>
      <p>每 {settings.refreshIntervalMinutes} 分钟刷新一次推荐</p>
    </div>
    <div class="interval-control">
      <input
        id="refresh-interval"
        type="range"
        min="1"
        max="60"
        bind:value={settings.refreshIntervalMinutes}
      />
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
      <span>分钟</span>
    </div>
  </section>

  <section class="setting-row startup-row">
    <div>
      <label for="launch-on-startup">开机自启动</label>
      <p>登录 Windows 后自动打开小助手</p>
    </div>
    <input
      id="launch-on-startup"
      class="switch"
      type="checkbox"
      bind:checked={settings.launchOnStartup}
    />
  </section>

  <section class="setting-row log-row">
    <div>
      <label for="log-path">日志文件</label>
      <p id="log-path">{logFilePath || "仅在桌面应用内可用"}</p>
    </div>
  </section>

  <footer>
    <button type="button" onclick={saveSettings} disabled={saving}>
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
    color: #202426;
    background: #eef0ef;
    font-family:
      "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI", system-ui, sans-serif;
  }

  :global(button),
  :global(input) {
    font: inherit;
  }

  .settings-shell {
    display: grid;
    gap: 14px;
    width: 100vw;
    min-height: 100vh;
    padding: 18px;
    background: #eef0ef;
    border: 1px solid rgb(23 34 31 / 10%);
  }

  header,
  .setting-row,
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }

  header {
    padding-bottom: 2px;
  }

  .kicker,
  p {
    margin: 0;
    color: #66706d;
    font-size: 12px;
    line-height: 1.4;
  }

  .kicker {
    color: #14734f;
    font-weight: 700;
  }

  h1 {
    margin: 2px 0 0;
    font-size: 18px;
    line-height: 1.1;
    letter-spacing: 0;
  }

  .status {
    min-width: 72px;
    color: #14734f;
    font-size: 12px;
    text-align: right;
  }

  .status.error {
    color: #9a4d2c;
  }

  .setting-row {
    min-height: 72px;
    border: 1px solid rgb(15 23 20 / 7%);
    border-radius: 8px;
    padding: 13px 14px;
    background: #ffffff;
    box-shadow: 0 5px 14px rgb(22 30 27 / 9%);
  }

  label {
    display: block;
    margin-bottom: 5px;
    color: #242b28;
    font-size: 14px;
    font-weight: 800;
  }

  .interval-control {
    display: grid;
    grid-template-columns: 92px 52px 28px;
    align-items: center;
    gap: 8px;
    color: #4e5a56;
    font-size: 12px;
  }

  input[type="range"] {
    accent-color: #20b47c;
  }

  .number-input {
    width: 52px;
    height: 30px;
    border: 1px solid rgb(32 180 124 / 26%);
    border-radius: 6px;
    color: #1f2b27;
    text-align: center;
    background: #f8fbfa;
  }

  .startup-row {
    min-height: 68px;
  }

  .log-row {
    min-height: 62px;
  }

  .log-row p {
    max-width: 300px;
    overflow-wrap: anywhere;
  }

  .switch {
    width: 42px;
    height: 22px;
    accent-color: #20b47c;
  }

  footer {
    justify-content: flex-end;
  }

  button {
    min-width: 96px;
    height: 32px;
    border: 1px solid rgb(32 180 124 / 22%);
    border-radius: 6px;
    padding: 0 14px;
    color: #ffffff;
    background: #20b47c;
    cursor: pointer;
  }

  button:disabled {
    cursor: wait;
    opacity: 0.62;
  }
</style>

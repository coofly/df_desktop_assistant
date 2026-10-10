<script lang="ts">
  // 文件说明：显示经过内置 NTP 服务校准的北京时间，并周期性重新选源。
  import { invoke } from "@tauri-apps/api/core";

  // 描述后端返回的北京时间校准结果。
  type TimeSyncSnapshot = {
    server: string;
    offsetMs: number;
    roundTripMs: number;
    syncedAtUnixMs: number;
  };

  // 保存北京时间校准结果，失败时保留上一次成功校准。
  let timeSync = $state<TimeSyncSnapshot | null>(null);
  // 保存当前北京时间文本，每秒边界前提前刷新。
  let beijingTime = $state(formatBeijingTime(Date.now()));
  // 标记是否尝试过时间校准。
  let timeSyncAttempted = $state(false);
  // 控制窗口内校准诊断提示的显示。
  let tooltipVisible = $state(false);
  // 保存时钟刷新与校准定时器句柄。
  let clockTimer: ReturnType<typeof window.setInterval> | undefined;
  let timeSyncTimer: ReturnType<typeof window.setInterval> | undefined;

  // 将 Unix 毫秒时间格式化为北京时间时分秒。
  function formatBeijingTime(timestampMs: number) {
    return new Intl.DateTimeFormat("zh-CN", {
      timeZone: "Asia/Shanghai",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23"
    }).format(new Date(timestampMs));
  }

  // 根据本机时间和校准偏差计算当前北京时间。
  function updateBeijingTime() {
    beijingTime = formatBeijingTime(Date.now() + (timeSync?.offsetMs ?? 0));
  }

  // 返回时钟状态文本。
  function timeSyncStatus() {
    if (timeSync) {
      return "已校准";
    }

    return timeSyncAttempted ? "本机时间" : "校准中";
  }

  // 构造时钟悬停时显示的诊断信息。
  function timeSyncTitle() {
    if (!timeSync) {
      return "北京时间校准中，当前暂显示本机时间";
    }

    const sign = timeSync.offsetMs >= 0 ? "+" : "";
    return `已使用 ${timeSync.server}，偏差 ${sign}${timeSync.offsetMs}ms，往返 ${timeSync.roundTripMs}ms`;
  }

  // 通过后端选择最快可用的内置 NTP 服务。
  async function syncBeijingTime() {
    timeSyncAttempted = true;

    try {
      timeSync = await invoke<TimeSyncSnapshot>("sync_beijing_time");
      updateBeijingTime();
    } catch {
      updateBeijingTime();
    }
  }

  $effect(() => {
    syncBeijingTime();
    clockTimer = window.setInterval(updateBeijingTime, 250);
    timeSyncTimer = window.setInterval(syncBeijingTime, 10 * 60 * 1000);

    return () => {
      if (clockTimer) {
        window.clearInterval(clockTimer);
      }
      if (timeSyncTimer) {
        window.clearInterval(timeSyncTimer);
      }
    };
  });
</script>

<div
  class="beijing-clock"
  role="status"
  aria-label={`${beijingTime}，${timeSyncStatus()}`}
  onmouseenter={() => (tooltipVisible = true)}
  onmouseleave={() => (tooltipVisible = false)}
>
  <span class="clock-label">北京时间</span>
  <strong>{beijingTime}</strong>
  <span class:unsynced={!timeSync} class="clock-status">{timeSyncStatus()}</span>
  {#if tooltipVisible}
    <span class="clock-tooltip">{timeSyncTitle()}</span>
  {/if}
</div>

<style>
  .beijing-clock {
    position: relative;
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    min-width: 178px;
    padding-left: 10px;
    border-left: 1px solid rgb(255 255 255 / 24%);
    color: #ffffff;
    white-space: nowrap;
  }

  .clock-label,
  .clock-status {
    color: #cfd3d7;
    font-size: 11px;
    line-height: 1.2;
  }

  .beijing-clock strong {
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 16px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0;
    line-height: 1;
  }

  .clock-status {
    color: #9bd0a5;
  }

  .clock-status.unsynced {
    color: #e2bb78;
  }

  .clock-tooltip {
    position: absolute;
    top: calc(100% + 7px);
    right: 0;
    z-index: 20;
    width: max-content;
    max-width: none;
    padding: 6px 8px;
    border: 1px solid #50545a;
    color: #e9ecef;
    background: #202326;
    box-shadow: 0 3px 8px rgb(0 0 0 / 28%);
    font-size: 11px;
    line-height: 1.35;
    pointer-events: none;
    text-align: left;
    white-space: nowrap;
  }
</style>

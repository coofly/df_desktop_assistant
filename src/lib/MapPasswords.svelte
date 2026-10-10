<script lang="ts">
  // 文件说明：展示六项每日地图密码并独立定时刷新，失败时静默保留数据。
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  // 密码卡片只展示地图名称和字符串密码。
  type MapPassword = {
    id: string;
    mapName: string;
    password: string;
  };

  // 后端返回最后成功的数据和获取时间。
  type MapPasswordSnapshot = {
    passwords: MapPassword[];
    refreshedAt: number | null;
  };

  let { loading = $bindable(false) }: { loading?: boolean } = $props();
  const refreshIntervalMs = 15 * 60 * 1000;
  let snapshot = $state<MapPasswordSnapshot>({
    passwords: [
      { id: "db", mapName: "零号大坝", password: "----" },
      { id: "cgxg", mapName: "长弓溪谷", password: "----" },
      { id: "bks", mapName: "巴克什", password: "----" },
      { id: "htjd", mapName: "航天基地", password: "----" },
      { id: "cxjy", mapName: "潮汐监狱", password: "----" },
      { id: "az3", mapName: "AZ3", password: "----" }
    ],
    refreshedAt: null
  });
  let refreshInFlight: Promise<void> | undefined;

  // 将最后成功获取时间格式化为与推荐标题一致的时分秒。
  function formatRefreshTime(timestamp: number | null) {
    return timestamp === null
      ? "未刷新"
      : new Intl.DateTimeFormat("zh-CN", {
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit"
        }).format(new Date(timestamp));
  }

  // 供共用刷新按钮调用，重入时等待当前请求完成。
  export function refresh(): Promise<void> {
    if (refreshInFlight) {
      return refreshInFlight;
    }
    loading = true;
    refreshInFlight = invoke<MapPasswordSnapshot>("get_map_passwords")
      .then((next) => {
        snapshot = next;
      })
      .catch(() => {
        // 调用失败时保持已有密码和成功时间，不显示失败提示。
      })
      .finally(() => {
        loading = false;
        refreshInFlight = undefined;
      });
    return refreshInFlight;
  }

  onMount(() => {
    refresh();
    const timer = window.setInterval(refresh, refreshIntervalMs);
    return () => window.clearInterval(timer);
  });
</script>

<section class="map-passwords" aria-labelledby="map-password-title">
  <header class="section-heading">
    <span class="rail"></span>
    <h2 id="map-password-title">每日地图密码</h2>
    <span class="updated-time">更新时间 {formatRefreshTime(snapshot.refreshedAt)}</span>
  </header>
  <div class="password-list" aria-busy={loading}>
    {#each snapshot.passwords as map (map.id)}
      <article class="password-card" aria-label={map.mapName}>
        <span class="map-name">{map.mapName}</span>
        <strong class="password">{map.password}</strong>
      </article>
    {/each}
  </div>
</section>

<style>
  .map-passwords {
    display: grid;
    grid-template-rows: 38px 64px;
    gap: 10px;
    min-width: 0;
  }

  .section-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 4px 8px;
    border: 1px solid #50545a;
    background: rgb(0 0 0 / 30%);
  }

  .rail {
    width: 8px;
    height: 24px;
    border: 1px solid #303336;
    background: rgb(23 25 26 / 12%);
  }

  h2 {
    margin: 0;
    color: #ffffff;
    font-size: 15px;
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: 0;
  }

  .updated-time {
    position: relative;
    top: 1px;
    color: #cfd3d7;
    font-size: 12px;
    line-height: 1.2;
    white-space: nowrap;
  }

  .password-list {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 14px;
  }

  .password-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    min-width: 0;
    border: 1px solid #50545a;
    border-radius: 0;
    background: rgb(0 0 0 / 30%);
  }

  .map-name {
    color: #cfd3d7;
    font-size: 12px;
    line-height: 1.2;
    white-space: nowrap;
  }

  .password {
    color: #ffffff;
    font-family: "Consolas", monospace;
    font-size: 24px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
    letter-spacing: 0;
  }
</style>

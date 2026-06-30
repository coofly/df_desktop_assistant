<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  type CraftRecommendation = {
    id: string;
    station: string;
    category: string;
    itemName: string;
    imageUrl?: string | null;
    currentProfit: number;
    idealPrice: number;
    sellTime: string;
    accent: string;
  };

  type AssistantSnapshot = {
    locked: boolean;
    updatedAt: string;
    recommendations: CraftRecommendation[];
  };

  type AssistantSettings = {
    refreshIntervalMinutes: number;
    launchOnStartup: boolean;
  };

  type SortMode = "profit" | "station";

  const stationOrder = ["技术中心", "工作台", "制药台", "防具台"];

  const previewSnapshot: AssistantSnapshot = {
    locked: false,
    updatedAt: "浏览器预览数据",
    recommendations: [
      {
        id: "tech-x25u",
        station: "技术中心",
        category: "工具",
        itemName: "X25U斜侧战斗握把",
        imageUrl: null,
        currentProfit: 6027,
        idealPrice: 35699,
        sellTime: "晚上11点",
        accent: "#6f5a91"
      },
      {
        id: "workbench-45-70",
        station: "工作台",
        category: "弹药",
        itemName: "45-70 Govt FTX",
        imageUrl: null,
        currentProfit: 36888,
        idealPrice: 5795,
        sellTime: "晚上11点",
        accent: "#a77452"
      },
      {
        id: "pharma-advanced-kit",
        station: "制药台",
        category: "医疗",
        itemName: "高级护甲维修组合",
        imageUrl: null,
        currentProfit: 5227,
        idealPrice: 328570,
        sellTime: "晚上11点",
        accent: "#a84d50"
      },
      {
        id: "armor-helmet",
        station: "防具台",
        category: "防具",
        itemName: "精英防弹背心",
        imageUrl: null,
        currentProfit: 23378,
        idealPrice: 336490,
        sellTime: "晚上11点",
        accent: "#a97752"
      }
    ]
  };

  let snapshot = $state<AssistantSnapshot>(previewSnapshot);
  let sortMode = $state<SortMode>("profit");
  let loading = $state(true);
  let error = $state("");
  let refreshedAt = $state("未刷新");
  let failedImages = $state<Set<string>>(new Set());
  let refreshIntervalMinutes = $state(5);
  let refreshTimer: ReturnType<typeof window.setInterval> | undefined;
  let unlistenSettings: UnlistenFn | undefined;

  const sortedRecommendations = $derived(
    [...snapshot.recommendations].sort((left, right) => {
      if (sortMode === "profit") {
        return right.currentProfit - left.currentProfit;
      }

      return stationOrder.indexOf(left.station) - stationOrder.indexOf(right.station);
    })
  );

  function formatNumber(value: number) {
    return new Intl.NumberFormat("zh-CN").format(value);
  }

  function formatNow() {
    return new Intl.DateTimeFormat("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit"
    }).format(new Date());
  }

  function clampInterval(value: number) {
    if (Number.isNaN(value)) {
      return 5;
    }

    return Math.min(60, Math.max(1, Math.round(value)));
  }

  async function writeLog(message: string) {
    try {
      await invoke("write_frontend_log", { source: "主面板", message });
    } catch {
      // Browser preview cannot call the Tauri log command.
    }
  }

  function scheduleRefresh() {
    if (refreshTimer) {
      window.clearInterval(refreshTimer);
    }

    writeLog(`已设置自动刷新定时器，每 ${refreshIntervalMinutes} 分钟刷新一次`);
    refreshTimer = window.setInterval(
      () => {
        writeLog("自动刷新定时器触发，准备重新获取推荐");
        loadRecommendations();
      },
      refreshIntervalMinutes * 60 * 1000
    );
  }

  async function loadRecommendations() {
    loading = true;
    error = "";
    writeLog("开始获取制作推荐");

    try {
      snapshot = await invoke<AssistantSnapshot>("get_recommendations");
      refreshedAt = formatNow();
      writeLog(`制作推荐加载成功，推荐数量：${snapshot.recommendations.length}，数据状态：${snapshot.updatedAt}`);
    } catch (cause) {
      snapshot = previewSnapshot;
      refreshedAt = formatNow();
      error = cause instanceof Error ? cause.message : "当前使用浏览器预览数据";
      writeLog(`制作推荐加载失败，已使用预览数据：${error}`);
    } finally {
      loading = false;
    }
  }

  async function loadSettings() {
    try {
      const settings = await invoke<AssistantSettings>("get_settings");
      refreshIntervalMinutes = clampInterval(settings.refreshIntervalMinutes);
      writeLog(`设置加载成功，刷新间隔：${refreshIntervalMinutes} 分钟`);
    } catch {
      refreshIntervalMinutes = 5;
      writeLog("设置加载失败，使用默认刷新间隔：5 分钟");
    }

    scheduleRefresh();
  }

  function applySettings(settings: AssistantSettings) {
    refreshIntervalMinutes = clampInterval(settings.refreshIntervalMinutes);
    writeLog(`收到设置更新事件，刷新间隔调整为 ${refreshIntervalMinutes} 分钟`);
    scheduleRefresh();
  }

  async function setLocked(locked: boolean) {
    try {
      snapshot = await invoke<AssistantSnapshot>("set_lock_state", { locked });
      writeLog(`窗口位置已${locked ? "锁定" : "解锁"}`);
    } catch {
      snapshot = { ...snapshot, locked };
      writeLog(`锁定状态更新失败，预览状态改为：${locked ? "锁定" : "解锁"}`);
    }
  }

  async function beginDrag() {
    if (snapshot.locked) {
      return;
    }

    try {
      await invoke("start_window_drag");
      writeLog("用户开始拖动窗口");
    } catch {
      // Browser preview cannot call the Tauri window command.
    }
  }

  function markImageFailed(id: string) {
    failedImages = new Set([...failedImages, id]);
    writeLog(`物品图片加载失败，已显示文字占位：${id}`);
  }

  $effect(() => {
    loadRecommendations();
    loadSettings();
    import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<AssistantSettings>("settings-updated", (event) => {
          applySettings(event.payload);
        })
      )
      .then((unlisten) => {
        unlistenSettings = unlisten;
      })
      .catch(() => {
        unlistenSettings = undefined;
      });

    return () => {
      if (refreshTimer) {
        window.clearInterval(refreshTimer);
      }

      if (unlistenSettings) {
        unlistenSettings();
      }
    };
  });
</script>

<svelte:head>
  <title>三角洲行动特勤处制作助手</title>
</svelte:head>

<main class="assistant-shell">
  <header
    class:locked={snapshot.locked}
    class="top-bar"
    role="button"
    tabindex="0"
    aria-label="拖动窗口"
    onpointerdown={beginDrag}
  >
    <div class="title-block">
      <span class="rail"></span>
      <div>
        <h1>特勤处制作产物推荐</h1>
        <p>{snapshot.updatedAt} · 本地刷新 {refreshedAt} · 每 {refreshIntervalMinutes} 分钟自动刷新</p>
      </div>
    </div>

    <div
      class="controls"
      role="group"
      aria-label="窗口和推荐控制"
      onpointerdown={(event) => event.stopPropagation()}
    >
      <button
        class:active={sortMode === "profit"}
        type="button"
        title="按利润排序"
        onclick={() => {
          sortMode = sortMode === "profit" ? "station" : "profit";
          writeLog(`排序方式切换为：${sortMode === "profit" ? "小时利润" : "工作台顺序"}`);
        }}
      >
        {sortMode === "profit" ? "小时利润" : "工作台序"}
      </button>
      <button type="button" title="刷新推荐" onclick={loadRecommendations} disabled={loading}>
        {loading ? "刷新中" : "刷新"}
      </button>
      <button
        class:locked={snapshot.locked}
        type="button"
        title={snapshot.locked ? "解锁窗口位置" : "锁定窗口位置"}
        onclick={() => setLocked(!snapshot.locked)}
      >
        {snapshot.locked ? "已锁定" : "未锁定"}
      </button>
    </div>
  </header>

  {#if error}
    <p class="status">{error}</p>
  {/if}

  <section class="recommendations" aria-label="制作推荐列表">
    {#each sortedRecommendations as recommendation}
      <article class="card">
        <div class="meta">
          <span class="station">{recommendation.station}</span>
          <span class="category">{recommendation.category}</span>
        </div>

        <dl>
          <div>
            <dt>当前利润</dt>
            <dd>{formatNumber(recommendation.currentProfit)}</dd>
          </div>
          <div>
            <dt>理想售价</dt>
            <dd>{formatNumber(recommendation.idealPrice)}</dd>
          </div>
          <div>
            <dt>卖</dt>
            <dd>{recommendation.sellTime}</dd>
          </div>
        </dl>

        <div class="item-panel" style={`--accent: ${recommendation.accent}`}>
          <div class="visual-slot">
            {#if recommendation.imageUrl && !failedImages.has(recommendation.id)}
              <img
                class="item-image"
                src={recommendation.imageUrl}
                alt=""
                loading="lazy"
                onerror={() => markImageFailed(recommendation.id)}
              />
            {:else}
              <div class="item-mark">
                {recommendation.itemName.slice(0, 2)}
              </div>
            {/if}
          </div>
          <strong>{recommendation.itemName}</strong>
        </div>
      </article>
    {/each}
  </section>
</main>

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(body) {
    margin: 0;
    min-width: 860px;
    overflow: hidden;
    color: #202426;
    background: transparent;
    font-family:
      "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI", system-ui, sans-serif;
  }

  :global(button) {
    font: inherit;
  }

  .assistant-shell {
    width: 100vw;
    height: 100vh;
    padding: 10px 12px 12px;
    background: #eef0ef;
    border: 1px solid rgb(23 34 31 / 10%);
    box-shadow: 0 18px 44px rgb(16 24 22 / 18%);
  }

  .top-bar {
    display: flex;
    min-height: 48px;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 2px 0 10px;
    cursor: grab;
    user-select: none;
  }

  .top-bar.locked {
    cursor: default;
  }

  .title-block {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 300px;
  }

  .rail {
    width: 4px;
    height: 40px;
    background: #20b47c;
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    line-height: 1.2;
    letter-spacing: 0;
  }

  p {
    margin: 4px 0 0;
    color: #66706d;
    font-size: 12px;
    line-height: 1.2;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  button {
    min-width: 72px;
    height: 28px;
    border: 1px solid rgb(32 180 124 / 22%);
    border-radius: 999px;
    padding: 0 12px;
    color: #14734f;
    background: #ffffff;
    box-shadow: none;
    cursor: pointer;
  }

  button:hover {
    border-color: rgb(32 180 124 / 44%);
  }

  button.active,
  button.locked {
    color: #ffffff;
    background: #20b47c;
  }

  button:disabled {
    cursor: wait;
    opacity: 0.62;
  }

  .status {
    position: absolute;
    top: 50px;
    right: 14px;
    margin: 0;
    color: #8d5a2e;
    font-size: 12px;
  }

  .recommendations {
    display: grid;
    grid-template-columns: repeat(4, minmax(200px, 1fr));
    gap: 14px;
  }

  .card {
    display: grid;
    grid-template-columns: minmax(124px, 1fr) 118px;
    min-height: 140px;
    overflow: hidden;
    border: 1px solid rgb(15 23 20 / 6%);
    border-radius: 8px;
    background: #ffffff;
    box-shadow: 0 5px 14px rgb(22 30 27 / 12%);
  }

  .meta,
  dl {
    grid-column: 1;
    padding-left: 16px;
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding-top: 16px;
  }

  .station {
    font-size: 13px;
    font-weight: 700;
    color: #333b38;
  }

  .category {
    width: fit-content;
    border-radius: 999px;
    padding: 2px 8px;
    color: #49615a;
    background: #edf3f1;
    font-size: 11px;
  }

  dl {
    margin: 10px 0 0;
  }

  dl div {
    display: flex;
    gap: 5px;
    align-items: baseline;
    min-height: 20px;
  }

  dt,
  dd {
    margin: 0;
    font-size: 12px;
  }

  dt {
    color: #37413d;
  }

  dd {
    color: #1e2925;
    font-weight: 800;
  }

  .item-panel {
    grid-row: 1 / span 2;
    grid-column: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-width: 0;
    padding: 12px;
    color: #ffffff;
    background:
      radial-gradient(circle at 50% 34%, rgb(255 255 255 / 24%), transparent 35%),
      var(--accent);
  }

  .visual-slot {
    position: relative;
    display: grid;
    width: 76px;
    height: 72px;
    place-items: center;
  }

  .item-mark {
    display: grid;
    width: 58px;
    height: 42px;
    place-items: center;
    border-radius: 5px;
    color: rgb(255 255 255 / 88%);
    background: rgb(12 18 16 / 22%);
    font-weight: 900;
  }

  .item-image {
    display: block;
    width: 72px;
    height: 72px;
    object-fit: contain;
    filter: drop-shadow(0 8px 10px rgb(0 0 0 / 20%));
  }


  strong {
    display: block;
    max-width: 100%;
    overflow-wrap: anywhere;
    text-align: center;
    font-size: 12px;
    line-height: 1.25;
  }

  @media (max-width: 980px) {
    .recommendations {
      grid-template-columns: repeat(2, minmax(240px, 1fr));
    }

    .assistant-shell {
      height: auto;
      min-height: 100vh;
      overflow: auto;
    }
  }
</style>

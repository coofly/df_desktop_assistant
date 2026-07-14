<script lang="ts">
  // 文件说明：主面板展示制作推荐、刷新状态和窗口锁定控制。
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  // 制作推荐条目的前端展示结构。
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

  // 主面板一次推荐数据快照。
  type AssistantSnapshot = {
    locked: boolean;
    updatedAt: string;
    recommendations: CraftRecommendation[];
  };

  // 主面板关心的助手设置结构。
  type AssistantSettings = {
    refreshIntervalMinutes: number;
    launchOnStartup: boolean;
    mainWindowOpacityPercent: number;
  };

  // 浏览器预览或后端失败时使用的默认推荐快照。
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

  // 当前面板展示的推荐快照。
  let snapshot = $state<AssistantSnapshot>(previewSnapshot);
  // 标记推荐刷新是否处于加载中。
  let loading = $state(true);
  // 保存当前页面错误提示。
  let error = $state("");
  // 记录最近一次刷新时间文本。
  let refreshedAt = $state("未刷新");
  // 记录加载失败的物品图片 ID。
  let failedImages = $state<Set<string>>(new Set());
  // 保存自动刷新间隔分钟数。
  let refreshIntervalMinutes = $state(5);
  // 保存主面板透明度百分比。
  let mainWindowOpacityPercent = $state(5);
  // 防止自动刷新和手动刷新重入。
  let refreshInFlight = false;
  // 标记是否已经拿到过后端推荐数据。
  let hasLoadedRecommendations = false;
  // 保存自动刷新定时器句柄。
  let refreshTimer: ReturnType<typeof window.setInterval> | undefined;
  // 保存设置事件监听的取消函数。
  let unlistenSettings: UnlistenFn | undefined;

  // 将数值格式化为中文分组数字。
  function formatNumber(value: number) {
    return new Intl.NumberFormat("zh-CN").format(value);
  }

  // 返回当前时间的时分秒文本。
  function formatNow() {
    return new Intl.DateTimeFormat("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit"
    }).format(new Date());
  }

  // 将刷新间隔限制在允许范围内。
  function clampInterval(value: number) {
    if (Number.isNaN(value)) {
      return 5;
    }

    return Math.min(60, Math.max(1, Math.round(value)));
  }

  // 将面板透明度限制在允许范围内。
  function clampOpacity(value: number) {
    if (Number.isNaN(value)) {
      return 5;
    }

    return Math.min(100, Math.max(5, Math.round(value)));
  }

  // 生成主面板背景透明色。
  function panelBackground() {
    return `rgba(239, 240, 238, ${mainWindowOpacityPercent / 100})`;
  }

  // 将前端日志转发给 Tauri 后端。
  async function writeLog(message: string) {
    try {
      await invoke("write_frontend_log", { source: "主面板", message });
    } catch {
      // Browser preview cannot call the Tauri log command.
    }
  }

  // 按当前设置重建自动刷新定时器。
  function scheduleRefresh() {
    if (refreshTimer) {
      window.clearInterval(refreshTimer);
    }

    refreshTimer = window.setInterval(
      () => {
        loadRecommendations();
      },
      refreshIntervalMinutes * 60 * 1000
    );
  }

  // 从后端加载制作推荐并处理失败兜底。
  async function loadRecommendations() {
    if (refreshInFlight) {
      writeLog("上一次制作推荐刷新仍在进行，本轮自动刷新已跳过");
      return;
    }

    refreshInFlight = true;
    loading = true;
    error = "";
    writeLog("开始刷新制作推荐");

    try {
      snapshot = await invoke<AssistantSnapshot>("get_recommendations");
      failedImages = new Set();
      refreshedAt = formatNow();
      hasLoadedRecommendations = true;
      writeLog(`制作推荐刷新完成：${snapshot.updatedAt}`);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : "当前刷新失败";
      if (hasLoadedRecommendations) {
        error = `制作推荐刷新失败，已保留上一次数据：${message}`;
        writeLog(error);
      } else {
        snapshot = previewSnapshot;
        refreshedAt = formatNow();
        error = `制作推荐初始化失败，当前使用浏览器预览数据：${message}`;
        writeLog(error);
      }
    } finally {
      loading = false;
      refreshInFlight = false;
    }
  }

  // 从后端加载助手设置并启动自动刷新。
  async function loadSettings() {
    try {
      const settings = await invoke<AssistantSettings>("get_settings");
      refreshIntervalMinutes = clampInterval(settings.refreshIntervalMinutes);
      mainWindowOpacityPercent = clampOpacity(settings.mainWindowOpacityPercent);
    } catch {
      refreshIntervalMinutes = 5;
      mainWindowOpacityPercent = 5;
      writeLog("设置加载失败，使用默认刷新间隔：5 分钟");
    }

    scheduleRefresh();
  }

  // 应用设置窗口广播过来的新设置。
  function applySettings(settings: AssistantSettings) {
    refreshIntervalMinutes = clampInterval(settings.refreshIntervalMinutes);
    mainWindowOpacityPercent = clampOpacity(settings.mainWindowOpacityPercent);
    scheduleRefresh();
  }

  // 切换主窗口锁定状态。
  async function setLocked(locked: boolean) {
    try {
      const nextLocked = await invoke<boolean>("set_lock_state", { locked });
      snapshot = { ...snapshot, locked: nextLocked };
    } catch {
      snapshot = { ...snapshot, locked };
      writeLog(`锁定状态更新失败，预览状态改为：${locked ? "锁定" : "解锁"}`);
    }
  }

  // 在未锁定时启动主窗口拖动。
  async function beginDrag(event: PointerEvent) {
    if (event.button !== 0) {
      return;
    }

    if (snapshot.locked) {
      return;
    }

    try {
      await invoke("start_window_drag");
    } catch {
      // Browser preview cannot call the Tauri window command.
    }
  }

  // 在未锁定时启动主窗口右下角缩放。
  async function beginResize(event: PointerEvent) {
    if (event.button !== 0) {
      return;
    }

    if (snapshot.locked) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    try {
      await invoke("start_window_resize");
    } catch {
      // Browser preview cannot call the Tauri window command.
    }
  }

  // 打开标题栏右键菜单。
  async function openTitleBarMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();

    try {
      await invoke("show_title_bar_menu", {
        x: event.clientX,
        y: event.clientY
      });
    } catch {
      // Browser preview cannot call the Tauri menu command.
    }
  }

  // 标记指定物品图片加载失败。
  function markImageFailed(id: string) {
    failedImages = new Set([...failedImages, id]);
    writeLog(`物品图片加载失败，已显示文字占位：${id}`);
  }

  // 将后端返回的本地缓存路径转换为 WebView 可读取的资源地址。
  function resolveImageSource(imageUrl: string) {
    return /^https?:\/\//i.test(imageUrl) ? imageUrl : convertFileSrc(imageUrl);
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

<main class="assistant-shell" style={`--panel-bg: ${panelBackground()}`}>
  <header
    class:locked={snapshot.locked}
    class="top-bar"
    role="button"
    tabindex="0"
    aria-label="拖动窗口"
    onpointerdown={beginDrag}
    oncontextmenu={openTitleBarMenu}
  >
    <div class="title-block">
      <span class="rail"></span>
      <h1>特勤处制作产物推荐</h1>
      <span class="updated-time">更新时间 {refreshedAt}</span>
    </div>

    <div
      class="controls"
      role="group"
      aria-label="窗口和推荐控制"
      onpointerdown={(event) => event.stopPropagation()}
    >
      <button
        class="icon-button"
        type="button"
        title="刷新推荐"
        aria-label="刷新推荐"
        onclick={loadRecommendations}
        disabled={loading}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M21 12a9 9 0 0 1-15.3 6.4" />
          <path d="M3 12A9 9 0 0 1 18.3 5.6" />
          <path d="M18 2v4h-4" />
          <path d="M6 22v-4h4" />
        </svg>
      </button>
      <button
        class:locked={snapshot.locked}
        class="icon-button"
        type="button"
        title={snapshot.locked ? "解锁窗口位置" : "锁定窗口位置"}
        aria-label={snapshot.locked ? "解锁窗口位置" : "锁定窗口位置"}
        onclick={() => setLocked(!snapshot.locked)}
      >
        {#if snapshot.locked}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <rect x="5" y="11" width="14" height="10" rx="1.5" />
            <path d="M8 11V7a4 4 0 0 1 8 0v4" />
            <path d="M12 15v2" />
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <rect x="5" y="11" width="14" height="10" rx="1.5" />
            <path d="M8 11V7a4 4 0 0 1 7.2-2.4" />
            <path d="M12 15v2" />
          </svg>
        {/if}
      </button>
    </div>
  </header>

  {#if error}
    <p class="status">{error}</p>
  {/if}

  <section class="recommendations" aria-label="制作推荐列表">
    {#each snapshot.recommendations as recommendation}
      <article class="card">
        <div class="meta">
          <span class="station">{recommendation.station}</span>
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
                src={resolveImageSource(recommendation.imageUrl)}
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

  <button
    class:locked={snapshot.locked}
    class="resize-grip"
    type="button"
    title={snapshot.locked ? "窗口位置已锁定" : "调整窗口大小"}
    aria-label={snapshot.locked ? "窗口位置已锁定" : "调整窗口大小"}
    onpointerdown={beginResize}
  ></button>
</main>

<style>
  :global(*) {
    box-sizing: border-box;
  }

  :global(body) {
    margin: 0;
    min-width: 910px;
    overflow: hidden;
    color: #17191a;
    background: transparent;
    font-family:
      "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI", system-ui, sans-serif;
  }

  :global(button) {
    font: inherit;
  }

  .assistant-shell {
    position: relative;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: 10px;
    width: 100vw;
    height: 100vh;
    padding: 10px;
    background: var(--panel-bg, rgb(239 240 238 / 85%));
    border: 1px solid #303336;
    box-shadow: none;
  }

  .resize-grip {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 18px;
    height: 18px;
    border: 0;
    padding: 0;
    opacity: 0;
    background: transparent;
    cursor: nwse-resize;
  }

  .resize-grip.locked {
    cursor: default;
    pointer-events: none;
  }

  .top-bar {
    display: flex;
    min-height: 38px;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    border: 1px solid #50545a;
    padding: 4px 8px;
    cursor: grab;
    user-select: none;
    background: rgb(0 0 0 / 30%);
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
    align-self: center;
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

  .updated-time {
    position: relative;
    top: 1px;
    color: #cfd3d7;
    font-size: 12px;
    line-height: 1.2;
    white-space: nowrap;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
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
    box-shadow: none;
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

  .icon-button:disabled {
    cursor: wait;
    opacity: 0.62;
  }

  .status {
    position: absolute;
    top: 50px;
    right: 14px;
    margin: 0;
    color: #4b3a2b;
    font-size: 12px;
  }

  .recommendations {
    display: grid;
    grid-template-columns: repeat(4, minmax(200px, 1fr));
    gap: 14px;
    min-height: 0;
  }

  .card {
    display: grid;
    grid-template-columns: minmax(124px, 1fr) 118px;
    min-height: 0;
    overflow: hidden;
    border: 1px solid #50545a;
    border-radius: 0;
    background: rgb(0 0 0 / 30%);
    box-shadow: none;
  }

  .meta,
  dl {
    grid-column: 1;
    padding-left: 16px;
  }

  .meta {
    display: flex;
    flex-direction: column;
    padding-top: 16px;
  }

  .station {
    font-size: 13px;
    font-weight: 700;
    color: #ffffff;
  }

  dl {
    margin: 12px 0 0;
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
    color: #cfd3d7;
  }

  dd {
    color: #ffffff;
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
    border-left: 1px solid #50545a;
    color: #ffffff;
    background: var(--accent);
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
    border: 1px solid #4b5055;
    border-radius: 0;
    color: rgb(255 255 255 / 88%);
    background: rgb(12 18 16 / 22%);
    font-weight: 900;
  }

  .item-image {
    display: block;
    width: 72px;
    height: 72px;
    object-fit: contain;
    filter: none;
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

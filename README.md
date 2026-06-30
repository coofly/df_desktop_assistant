# Delta Force Craft Assistant

三角洲行动特勤处制作产物推荐桌面小助手。

这个项目使用 Rust + Tauri 2 + SvelteKit 构建，用于把 `https://www.kkrb.net/?viewpage=view%2Foverview` 中的特勤处制作推荐整理成一个紧凑的桌面面板。当前初始化版本先使用结构化 mock 数据，重点完成桌面窗口、标题栏右键菜单、锁定位置和前端展示边界。

## MVP 功能

- 紧凑横向推荐面板，展示制作台、推荐产物、当前利润、理想售价和建议售卖时间。
- 不创建系统托盘图标，也不显示任务栏图标。
- 标题栏右键菜单支持设置、锁定位置、解锁位置、退出程序。
- 关闭主窗口时按常规窗口生命周期退出。
- 锁定后窗口位置固定，但推荐列表、刷新、排序等页面交互仍可使用。
- 前端通过 Tauri command 读取结构化推荐数据，后续可以平滑替换为真实网站数据源。

## 技术栈

- Rust 2021
- Tauri 2
- Svelte 5 / SvelteKit
- TypeScript
- Vite

## 开发命令

```powershell
npm install
npm run check
npm run tauri dev
```

Rust 侧单独检查：

```powershell
cd src-tauri
cargo check
cargo test
```

构建桌面包：

```powershell
npm run tauri build
```

## 目录结构

```text
src/
  routes/+page.svelte        # 桌面助手前端界面
src-tauri/
  src/lib.rs                 # Tauri commands、标题栏菜单、窗口生命周期、mock 推荐数据
  tauri.conf.json            # 桌面窗口和打包配置
docs/superpowers/
  specs/                     # 需求设计记录
  plans/                     # 实施计划记录
```

## 数据接入边界

当前推荐数据由 Rust 侧 `get_recommendations` command 返回，前端只消费 `CraftRecommendation` 结构：

- `station`: 制作台/部门
- `category`: 产物分类
- `itemName`: 推荐产物名称
- `currentProfit`: 当前利润
- `idealPrice`: 理想售价
- `sellTime`: 建议售卖时间
- `accent`: 卡片缩略图色值

后续接入 `kkrb.net` 时，建议保持这个 command contract 不变：

1. Rust 侧新增数据源模块，负责请求网页或已确认的内部接口。
2. 将外部字段归一化为 `CraftRecommendation`。
3. 缓存最近一次成功结果。
4. 网络或解析失败时返回缓存和状态提示，避免桌面助手空白。

## 标题栏菜单和锁定行为

标题栏右键菜单提供常用窗口操作：

- `设置`: 打开设置窗口。
- `锁定位置`: 固定窗口位置，防止误拖动。
- `解锁位置`: 允许通过顶部栏拖动窗口。
- `退出程序`: 真正退出应用。

锁定只影响窗口移动，不影响页面按钮、推荐卡片或刷新操作。

## Roadmap

- 接入真实 kkrb.net 数据源。
- 增加自动刷新间隔和失败状态。
- 加入标题栏菜单勾选态或状态提示。
- 支持开机自启、窗口位置持久化。
- 使用真实物品图标替换当前色块缩略图。

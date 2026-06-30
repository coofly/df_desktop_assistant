# AGENTS.md

## 项目定位

这是一个三角洲行动特勤处制作产物推荐桌面小助手，技术栈为 Rust + Tauri 2 + SvelteKit。第一屏应始终是可用的桌面助手面板，不要做营销页或无关 landing page。

## Windows PowerShell / SSH 规则

在 Windows PowerShell 中通过 ssh 执行远端多段 shell 脚本时，外层优先用单引号包住远端命令，避免本地 PowerShell 提前解析 `$变量`、分号、双引号和 `curl -w "%{...}"`。

不要用 Bash 习惯的 `\"` 来转义 PowerShell 字符串。

## 架构边界

- Rust/Tauri 负责系统能力：托盘图标、窗口显示隐藏、关闭隐藏到托盘、退出程序、锁定状态、数据获取与缓存。
- Svelte 负责展示和交互：推荐卡片、排序、刷新状态、锁定状态提示。
- 前端不要直接抓取或解析 `kkrb.net` 页面；真实数据接入应封装在 Rust command 后面。
- `get_recommendations` 的返回结构应保持稳定，方便 mock 数据和真实数据源互换。

## 托盘和窗口要求

- 程序启动必须创建系统托盘图标。
- 托盘菜单至少包含：显示/隐藏窗口、锁定位置、解锁位置、退出程序。
- 主窗口关闭按钮应隐藏到托盘，不应直接退出。
- 真正退出程序由托盘菜单触发。
- 锁定位置只禁止窗口拖动或误移动，不能禁用页面交互。

## 前端设计约定

- 保持小助手形态：紧凑、信息密度高、适合桌面边缘常驻。
- 避免大 hero、营销文案、装饰性背景和一屏无用介绍。
- 文案优先使用玩家能理解的词：制作台、当前利润、理想售价、售卖时间、锁定位置。
- 卡片布局要稳定，动态内容不能造成明显跳动。

## 开发与验证

常用命令：

```powershell
npm run check
npm run tauri dev
```

Rust 检查：

```powershell
cd src-tauri
cargo check
cargo test
```

如果首次运行需要下载 npm 或 Cargo 依赖，先说明原因再执行。

## Codex Windows 沙箱注意事项

- Windows 沙箱内可能出现 Schannel TLS 相关错误，影响 `curl.exe`、`Invoke-WebRequest`、Cargo 下载依赖等；这通常是 Codex 沙箱限制，不代表目标站点不可访问。需要时优先使用 OpenSSL/rustls 后端，或按权限流程申请提权运行。
- Windows 沙箱内 `npm`/`npx` shim 可能因路径隔离不可用；这通常不是项目依赖或用户机器环境损坏。验证前端时可改用本地 `node_modules` 入口，或按权限流程申请提权运行常规 `npm run check`。

## 文件与提交卫生

- 不要提交 `node_modules/`、`build/`、`.svelte-kit/`、`src-tauri/target/`。
- 不要提交 `.superpowers/` 视觉伴侣或 brainstorming 临时文件。
- 不要随意重命名项目目录、Tauri identifier 或包名，除非用户明确要求。
- 不要回滚用户已有改动；如果发现无关脏文件，保持原样。

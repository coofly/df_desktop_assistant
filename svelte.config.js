// 文件说明：配置 SvelteKit 使用静态适配器输出 Tauri 前端资源。
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
// 定义 SvelteKit 构建、预处理和 SPA fallback 配置。
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;

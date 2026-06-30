<script lang="ts">
  // 文件说明：全局布局负责禁用默认右键菜单并保留输入控件选中文本能力。
  // 保存当前路由页面内容渲染函数。
  let { children } = $props();

  // 阻止 WebView 默认右键菜单覆盖自定义标题栏菜单。
  function blockWebviewContextMenu(event: MouseEvent) {
    event.preventDefault();
  }
</script>

<svelte:window oncontextmenu={blockWebviewContextMenu} />

{@render children()}

<style>
  :global(body) {
    -webkit-user-select: none;
    user-select: none;
  }

  :global(input),
  :global(textarea),
  :global(select),
  :global([contenteditable="true"]) {
    -webkit-user-select: text;
    user-select: text;
  }
</style>

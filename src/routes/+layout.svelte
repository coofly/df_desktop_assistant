<script lang="ts">
  let { children } = $props();

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

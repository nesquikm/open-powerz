<script lang="ts">
  import type { Session } from "../session.svelte";
  import Notice from "./Notice.svelte";

  let { session }: { session: Session } = $props();
</script>

{#if session.error}
  <Notice tone="error" message={session.error.message} onDismiss={() => session.dismissError()}>
    {#if session.error.hint}
      <p class="hint"><code>{session.error.hint}</code></p>
    {/if}
  </Notice>
{/if}

<style>
  p.hint {
    margin: 0.25rem 0 0;
  }
  .hint code {
    user-select: text;
    white-space: pre-wrap;
  }
</style>

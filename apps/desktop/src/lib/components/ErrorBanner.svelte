<script lang="ts">
  import type { Session } from "../session.svelte";
  import { splitHint } from "../hint";
  import Notice from "./Notice.svelte";

  let { session }: { session: Session } = $props();
</script>

{#if session.error}
  <Notice tone="error" message={session.error.message} onDismiss={() => session.dismissError()}>
    {#if session.error.hint}
      <p class="hint">
        {#each splitHint(session.error.hint) as part, i (i)}{#if part.code}<code>{part.text}</code
            >{:else}{part.text}{/if}{/each}
      </p>
    {/if}
  </Notice>
{/if}

<style>
  p.hint {
    margin: 0.25rem 0 0;
    user-select: text;
  }
  .hint code {
    font-family: ui-monospace, monospace;
    white-space: pre-wrap;
  }
</style>

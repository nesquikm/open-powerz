<script lang="ts">
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();
</script>

{#if session.error}
  <div class="error-banner" role="alert">
    <div class="text">
      <p class="message">{session.error.message}</p>
      {#if session.error.hint}
        <p class="hint"><code>{session.error.hint}</code></p>
      {/if}
    </div>
    <button type="button" onclick={() => session.dismissError()}>Dismiss</button>
  </div>
{/if}

<style>
  .error-banner {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    margin: 0.75rem 1rem 0;
    padding: 0.625rem 0.875rem;
    border: 1px solid currentColor;
    border-radius: 0.5rem;
    min-width: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .message {
    margin: 0;
    font-weight: 600;
  }
  .hint {
    margin: 0.25rem 0 0;
    overflow-wrap: anywhere;
  }
  .hint code {
    user-select: text;
    white-space: pre-wrap;
  }
</style>

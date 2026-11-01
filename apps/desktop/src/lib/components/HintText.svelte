<script lang="ts">
  import { splitHint } from "../hint";

  /** An error's hint, inline: its backtick quotes become code. Nothing without a hint. */
  let { hint }: { hint: string | null } = $props();
</script>

{#each splitHint(hint ?? "") as part, i (i)}{#if part.code}<code>{part.text}</code
    >{:else}{part.text}{/if}{/each}

<style>
  code {
    font-family: ui-monospace, monospace;
    white-space: pre-wrap;
  }
</style>

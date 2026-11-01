<script lang="ts">
  import type { Session } from "../session.svelte";
  import Notice from "./Notice.svelte";

  let { session }: { session: Session } = $props();
</script>

{#if session.error}
  <Notice
    tone="error"
    message={session.error.message}
    hint={session.error.hint}
    onDismiss={() => session.dismissError()}
  />
{/if}

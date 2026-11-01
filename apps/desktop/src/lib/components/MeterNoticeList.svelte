<script lang="ts">
  import type { Snippet } from "svelte";
  import type { MeterNotice } from "../meterNotices.svelte";
  import HintText from "./HintText.svelte";
  import Notice from "./Notice.svelte";

  let {
    notices,
    onDismiss,
    fileActions,
  }: {
    /** One meter's notices (FR AXHK2K), oldest first. */
    notices: readonly MeterNotice[];
    onDismiss: (notice: MeterNotice) => void;
    /** The reveal and copy actions of a notice that speaks of a file. */
    fileActions: Snippet<[string]>;
  } = $props();
</script>

{#each notices as notice (notice)}
  <Notice tone={notice.tone} message={notice.message ?? null} onDismiss={() => onDismiss(notice)}>
    {#if notice.text}<p>
        {#if notice.tone === "error"}<HintText hint={notice.text} />{:else}{notice.text}{/if}
      </p>{/if}
    {#if notice.path !== null}
      {@render fileActions(notice.path)}
    {/if}
  </Notice>
{/each}

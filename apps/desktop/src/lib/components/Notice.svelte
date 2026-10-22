<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    tone,
    message = null,
    onDismiss,
    children,
  }: {
    /** `error` is announced at once (`role="alert"`), `info` politely. */
    tone: "error" | "info";
    /** Bold first line, when there is one. */
    message?: string | null;
    onDismiss: () => void;
    /** More lines under the message. */
    children?: Snippet;
  } = $props();
</script>

<div
  class="notice {tone}"
  role={tone === "error" ? "alert" : "status"}
  aria-live={tone === "error" ? undefined : "polite"}
>
  <div class="text">
    {#if message}<p class="message">{message}</p>{/if}
    {@render children?.()}
  </div>
  <button type="button" onclick={onDismiss}>Dismiss</button>
</div>

<style>
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    margin: 0.75rem 1rem 0;
    padding: 0.5rem 0.875rem;
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface);
    min-width: 0;
  }
  .notice.error {
    padding: 0.625rem 0.875rem;
    border-color: currentColor;
    background: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .text :where(:global(p)) {
    margin: 0;
  }
  .message {
    font-weight: 600;
  }
</style>

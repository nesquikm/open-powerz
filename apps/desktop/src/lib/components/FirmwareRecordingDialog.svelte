<script lang="ts">
  /**
   * Asked before a firmware update ends running sessions (FR AXHK2F, AC .4): a modal
   * dialog that names each session and says which are recording. It opens with its Keep
   * button focused, keeps Tab inside, and treats Escape as Keep.
   */
  import type { FlashHold } from "../backend";

  let {
    sessions,
    onKeep,
    onEnd,
  }: {
    /** The sessions the update would end. */
    sessions: FlashHold[];
    onKeep: () => void;
    onEnd: () => void;
  } = $props();

  const anyRecording = $derived(sessions.some((s) => s.recording));
  /** What the title and the buttons call what would end. */
  const noun = $derived(anyRecording ? "recording" : "sessions");

  function focusOnMount(el: HTMLElement): void {
    el.focus();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key !== "Tab") return;
    // Keep focus inside the dialog.
    const buttons = Array.from(
      (e.currentTarget as HTMLElement).querySelectorAll<HTMLButtonElement>("button"),
    );
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }
</script>

<!-- Escape keeps the sessions even when focus has left the dialog (a click on the backdrop). -->
<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onKeep();
    }
  }}
/>

<div class="backdrop">
  <div
    class="confirm panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="firmware-recording-title"
    aria-describedby="firmware-recording-text"
    tabindex="-1"
    onkeydown={onKey}
  >
    <h2 id="firmware-recording-title">End the running {noun}?</h2>
    <p id="firmware-recording-text">
      The update disconnects these meters, which ends their sessions.
      {#if anyRecording}Ending a recording saves the rows written so far.{/if}
    </p>
    <ul>
      {#each sessions as session (session.id)}
        <li>{session.label}{session.recording ? " (recording)" : ""}</li>
      {/each}
    </ul>
    <div class="actions">
      <button type="button" use:focusOnMount onclick={onKeep}>Keep {noun}</button>
      <button type="button" class="primary" onclick={onEnd}>End {noun} and update</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 60%, transparent);
  }
  .confirm {
    max-width: 24rem;
    padding: 1rem;
  }
  .confirm h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }
  .confirm p {
    margin: 0 0 1rem;
    font-size: 0.875rem;
  }
  .confirm ul {
    margin: 0 0 1rem;
    padding-left: 1.25rem;
    font-size: 0.875rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>

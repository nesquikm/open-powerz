<script lang="ts">
  import type { MessageSop } from "../backend";
  import {
    DISCOVER_IDENTITY_CABLE,
    formatLine,
    messageTypeError,
    parseDataObjects,
    type PdConsole,
  } from "../console.svelte";
  import TriggerConfirmDialog from "./TriggerConfirmDialog.svelte";

  /**
   * The PD message builder, the cable's e-marker details and the raw console (FR
   * DGSQ6T), on the open trigger session.
   */
  let { pdConsole }: { pdConsole: PdConsole } = $props();

  const TARGETS: readonly { value: MessageSop; label: string }[] = [
    { value: "sop", label: "SOP" },
    { value: "sop_prime", label: "SOP'" },
    { value: "sop_double_prime", label: "SOP''" },
  ];

  let target = $state<MessageSop>("sop");
  let msgType = $state(13);
  let objects = $state("");
  let objectsError = $state<string | null>(null);
  let typeError = $state<string | null>(null);
  let command = $state("");
  /** No send while one is in flight or waits for its confirmation. */
  const controlsOff = $derived(pdConsole.busy || pdConsole.pending !== null);

  /** Hex with `0x`, at least four digits. */
  function hex(n: number): string {
    return `0x${n.toString(16).toUpperCase().padStart(4, "0")}`;
  }

  async function sendBuilt(): Promise<void> {
    typeError = messageTypeError(msgType);
    if (typeError !== null) return;
    const dataObjects = parseDataObjects(objects);
    if (dataObjects === null) {
      objectsError = "Data objects are up to 7 hex words of at most 8 digits.";
      return;
    }
    objectsError = null;
    await pdConsole.sendMessage({ sop: target, msg_type: msgType, data_objects: dataObjects });
  }

  /** Scroll a newly shown reason into view, so it is seen next to the console. */
  function reveal(node: HTMLElement): void {
    node.scrollIntoView?.({ block: "nearest" });
  }

  async function sendLine(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    await clearAfter(() => pdConsole.sendLine(command));
  }

  /** Empties the field once its line went out, unless the user typed on meanwhile. */
  async function clearAfter(send: () => Promise<boolean>): Promise<void> {
    const line = command;
    if ((await send()) && command === line) command = "";
  }
</script>

<div class="console-sections">
  <section class="card" aria-labelledby="pd-builder-title">
    <h2 id="pd-builder-title">PD message builder</h2>
    <form
      class="builder"
      onsubmit={(e) => {
        e.preventDefault();
        void sendBuilt();
      }}
    >
      <label for="pd-builder-target">Target</label>
      <select id="pd-builder-target" bind:value={target}>
        {#each TARGETS as t (t.value)}
          <option value={t.value}>{t.label}</option>
        {/each}
      </select>
      <label for="pd-builder-type">Message type</label>
      <input
        id="pd-builder-type"
        type="number"
        min="0"
        max="31"
        step="1"
        aria-describedby={typeError ? "pd-builder-type-error" : undefined}
        bind:value={msgType}
      />
      {#if typeError}
        <p id="pd-builder-type-error" class="error" role="alert">{typeError}</p>
      {/if}
      <label for="pd-builder-objects">Data objects</label>
      <input
        id="pd-builder-objects"
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="empty: a control message"
        aria-describedby={objectsError ? "pd-builder-objects-error" : undefined}
        bind:value={objects}
      />
      {#if objectsError}
        <p id="pd-builder-objects-error" class="error" role="alert">{objectsError}</p>
      {/if}
      <div class="actions">
        <button type="submit" class="primary" disabled={controlsOff}>Send message</button>
        <button
          type="button"
          disabled={controlsOff}
          onclick={() => void pdConsole.sendMessage(DISCOVER_IDENTITY_CABLE)}
          >Discover cable identity</button
        >
      </div>
    </form>
  </section>

  <section class="card" aria-labelledby="pd-cable-title">
    <h2 id="pd-cable-title">Cable identity</h2>
    {#if pdConsole.cable}
      {@const c = pdConsole.cable}
      <dl class="facts">
        <dt>VID</dt>
        <dd>{hex(c.vid)}</dd>
        {#if c.pid !== null}
          <dt>PID</dt>
          <dd>{hex(c.pid)}</dd>
        {/if}
        <dt>Type</dt>
        <dd>{c.active ? "Active" : "Passive"}</dd>
        {#if c.max_voltage_v !== null}
          <dt>Max voltage</dt>
          <dd>{c.max_voltage_v} V</dd>
        {/if}
        {#if c.max_current_ma !== null}
          <dt>Max current</dt>
          <dd>{Number((c.max_current_ma / 1000).toFixed(2))} A</dd>
        {/if}
        <dt>EPR</dt>
        <dd>{c.epr_capable ? "EPR capable" : "Not EPR capable"}</dd>
      </dl>
    {:else}
      <p class="muted">No cable has answered yet.</p>
    {/if}
  </section>

  <section class="card console" aria-labelledby="pd-console-title">
    <h2 id="pd-console-title">Console</h2>
    <form class="line" onsubmit={(e) => void sendLine(e)}>
      <label for="pd-console-command">Command</label>
      <input
        id="pd-console-command"
        type="text"
        spellcheck="false"
        autocomplete="off"
        bind:value={command}
      />
      <button type="submit" disabled={controlsOff}>Send</button>
    </form>
    {#if pdConsole.error}
      <p class="error" role="alert" {@attach reveal}>
        {pdConsole.error.hint
          ? `${pdConsole.error.message} ${pdConsole.error.hint}`
          : pdConsole.error.message}
      </p>
    {/if}
    <div class="transcript" role="log" aria-label="Console transcript">
      {#each pdConsole.lines as line, i (i)}
        <div class={line.direction}>{formatLine(line)}</div>
      {/each}
    </div>
  </section>
</div>

{#if pdConsole.pending}
  <TriggerConfirmDialog
    voltageMv={0}
    text={pdConsole.pending.text}
    onCancel={() => pdConsole.cancel()}
    onConfirm={() => void clearAfter(() => pdConsole.confirm())}
  />
{/if}

<style>
  .console-sections {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(18rem, 1fr));
    gap: 1rem;
  }
  .card {
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    background: var(--surface);
    padding: 0.75rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .console {
    grid-column: 1 / -1;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  .builder {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.875rem;
  }
  .builder .actions,
  .builder .error {
    grid-column: 1 / -1;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.875rem;
  }
  .line input {
    flex: 1 1 12rem;
  }
  .transcript {
    min-height: 6rem;
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 0.8125rem;
    white-space: pre-wrap;
  }
  .facts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
  }
  .facts dd {
    margin: 0;
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .error {
    margin: 0;
  }
</style>

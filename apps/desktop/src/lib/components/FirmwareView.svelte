<script lang="ts">
  /**
   * The firmware update screen (FR V9SK55): the meter's mode and versions, the chosen
   * file's metadata, and Update, enabled only for an inspected file of the meter's model
   * (AC .2), behind a typed confirmation of the target version (AC .3), then each stage
   * with the write progress while the meter is held for the update (AC .4), and the
   * outcome: the version read back, or the error with the recovery steps (AC .5).
   */
  import {
    FLASH_STAGES,
    RECOVERY_STEPS,
    firmwareMeterRows,
    replugPrompt,
    stageNameLabel,
    type FirmwareMeter,
    type FirmwareUpdate,
  } from "../firmwareUpdate.svelte";
  import FirmwareConfirmDialog from "./FirmwareConfirmDialog.svelte";
  import FirmwareFileInfo from "./FirmwareFileInfo.svelte";

  let {
    firmware,
    meter,
  }: {
    /** The view's state, kept by the app across tab switches. */
    firmware: FirmwareUpdate;
    /** The meter the update would go to, null when none is picked. */
    meter: FirmwareMeter | null;
  } = $props();

  const shownMeter = $derived(firmware.meterShown(meter));
  const meterRows = $derived(shownMeter ? firmwareMeterRows(shownMeter) : []);
  const blocker = $derived(firmware.updateBlocker(meter));
  /** Where the update is in `FLASH_STAGES`, -1 before the first stage. */
  const stageIndex = $derived(
    firmware.stage === null ? -1 : FLASH_STAGES.indexOf(firmware.stage.stage),
  );
  /** Each stage's row in the progress list: done, current or still to come. */
  const stageRows = $derived(
    FLASH_STAGES.map((name, i) => ({
      name,
      label: stageNameLabel(name, firmware.writeProgress),
      state:
        i < stageIndex || firmware.phase === "done"
          ? "done"
          : i === stageIndex
            ? firmware.phase === "failed"
              ? "failed"
              : "current"
            : "pending",
    })),
  );
  const writePercent = $derived(
    firmware.writeProgress === null || firmware.writeProgress.total === 0
      ? 0
      : Math.round((firmware.writeProgress.done / firmware.writeProgress.total) * 100),
  );
  /** The progress region and the outcome's Close button, focus targets once the dialog closes. */
  let progressEl: HTMLElement | null = $state(null);
  let closeEl: HTMLButtonElement | null = $state(null);
  // The dialog gives focus back to Update, which is disabled while the update runs, so
  // focus would fall to the page: keep it on the screen, on the progress while the update
  // runs and on the outcome's Close button once it ends (AC .6).
  // The Close button takes focus only when the phase changes into done / failed, not
  // when the view mounts or is rebound with an outcome already showing.
  let lastPhase: string | undefined;
  $effect(() => {
    const phase = firmware.phase;
    // The first run (mount, or a rebound view) only records the phase.
    const changed = lastPhase !== undefined && phase !== lastPhase;
    lastPhase = phase;
    if (phase === "updating") progressEl?.focus();
    else if (changed && (phase === "done" || phase === "failed")) closeEl?.focus();
  });
  const STATE_TEXT: Record<string, string> = {
    done: "done",
    current: "in progress",
    failed: "failed",
    pending: "to do",
  };
</script>

<section class="meter" aria-label="Meter">
  <h3>Meter</h3>
  {#if shownMeter}
    <dl>
      {#each meterRows as row (row.label)}
        <dt>{row.label}</dt>
        <dd>{row.value}</dd>
      {/each}
    </dl>
  {:else}
    <p>No meter is picked.</p>
  {/if}
</section>

<div class="file">
  <button
    type="button"
    disabled={firmware.phase === "updating" || firmware.phase === "confirming"}
    onclick={() => void firmware.chooseFile()}>Choose file…</button
  >
  {#if firmware.fileError !== null}
    <p class="error" role="alert">{firmware.fileError}</p>
  {/if}
  {#if firmware.info !== null}
    <FirmwareFileInfo info={firmware.info} />
  {/if}
</div>

<div class="actions">
  <button
    type="button"
    disabled={blocker !== null || firmware.busy}
    onclick={() => firmware.requestUpdate(meter)}
    aria-describedby={blocker !== null ? "firmware-update-blocker" : undefined}>Update</button
  >
  {#if blocker !== null}
    <p id="firmware-update-blocker" class="blocker">{blocker}</p>
  {/if}
</div>

{#if firmware.phase === "updating" || firmware.phase === "done" || firmware.phase === "failed"}
  <section class="progress" aria-label="Update progress" tabindex="-1" bind:this={progressEl}>
    <h3>Update progress</h3>
    {#if firmware.replug}
      <p class="replug" role="status">
        {replugPrompt(firmware.replug)}
        <span class="reply">The meter's reply to the jump: {firmware.replug.reply}.</span>
      </p>
    {/if}
    <ol>
      {#each stageRows as row (row.name)}
        <li data-state={row.state} aria-current={row.state === "current" ? "step" : undefined}>
          {row.label} <span class="visually-hidden">({STATE_TEXT[row.state]})</span>
        </li>
      {/each}
    </ol>
    <progress
      aria-label="Write progress"
      max="100"
      value={writePercent}
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={writePercent}
    ></progress>
  </section>
{/if}

{#if firmware.phase === "done" && firmware.result !== null}
  <p class="outcome" role="status">
    {#if firmware.result.outcome === "verified"}
      The meter was updated to {firmware.result.version}.
    {:else}
      The image was accepted, but the meter stayed in its bootloader. Unplug it and plug it back in.
    {/if}
  </p>
  <button type="button" bind:this={closeEl} onclick={() => firmware.dismiss()}>Close</button>
{/if}

{#if firmware.phase === "failed" && firmware.error !== null}
  <div class="outcome error" role="alert">
    <p>
      {#if firmware.needsRecovery && firmware.failedStage !== null}Failed while: {stageNameLabel(
          firmware.failedStage,
          firmware.writeProgress,
        )}.
      {/if}
      {firmware.error.message}
    </p>
    {#if firmware.needsRecovery}
      <p>To recover the meter:</p>
      <ol>
        {#each RECOVERY_STEPS as step (step)}
          <li>{step}</li>
        {/each}
      </ol>
    {:else}
      <p>Nothing was sent to the meter.</p>
    {/if}
  </div>
  <button type="button" bind:this={closeEl} onclick={() => firmware.dismiss()}>Close</button>
{/if}

{#if firmware.phase === "confirming" && firmware.info !== null}
  <FirmwareConfirmDialog
    version={firmware.info.version}
    bind:confirmation={firmware.confirmation}
    confirmed={firmware.confirmed}
    onCancel={() => firmware.cancel()}
    onConfirm={() => void firmware.start(meter)}
  />
{/if}

<style>
  h3 {
    margin: 0 0 0.25rem;
    font-size: 1rem;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.25rem 1rem;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .file,
  .actions,
  .progress,
  .outcome {
    margin-top: 1rem;
  }
  .replug {
    margin: 0 0 0.5rem;
    font-weight: 600;
  }
  .replug .reply {
    display: block;
    font-weight: normal;
    color: var(--muted);
  }
  ol {
    margin: 0 0 0.5rem;
    padding-left: 1.25rem;
  }
  li[data-state="pending"] {
    color: var(--muted);
  }
  li[data-state="current"] {
    font-weight: 600;
  }
  li[data-state="failed"] {
    color: var(--danger);
  }
  progress {
    width: 100%;
    max-width: 24rem;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .file button {
    margin-bottom: 0.75rem;
  }
  .error {
    margin: 0 0 0.75rem;
    color: var(--danger);
  }
  .blocker {
    margin: 0.5rem 0 0;
    color: var(--muted);
  }
</style>

<script lang="ts">
  import type { StartupLogoState } from "../logo.svelte";
  import LogoConfirmDialog from "./LogoConfirmDialog.svelte";

  let { logo }: { logo: StartupLogoState } = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);

  let storedCanvas = $state<HTMLCanvasElement | null>(null);

  /** Draws base64 RGBA8888 `rgba` of `width × height` onto `target` 1:1, so it shows at
   * its real pixel size. */
  function drawRgba(
    target: HTMLCanvasElement | null,
    rgba: string,
    width: number,
    height: number,
  ): void {
    const ctx = target?.getContext("2d");
    if (!ctx) return;
    const raw = atob(rgba);
    const px = new Uint8ClampedArray(width * height * 4);
    for (let i = 0; i < px.length && i < raw.length; i++) px[i] = raw.charCodeAt(i);
    ctx.putImageData(new ImageData(px, width, height), 0, 0);
  }

  /** Draws the preview: the logo exactly as it would be written. */
  $effect(() => {
    const p = logo.preview;
    if (p) drawRgba(canvas, p.rgba, p.width, p.height);
  });

  /** Draws the stored logo, as read back from the meter. */
  $effect(() => {
    const a = logo.area;
    if (a && a.image !== null && a.width !== null && a.height !== null)
      drawRgba(storedCanvas, a.image, a.width, a.height);
  });

  /** What the meter's logo area holds, in words. */
  const areaLabel = $derived.by(() => {
    const a = logo.area;
    if (a === null) return null;
    if (a.status === "empty") return "The meter holds no logo (empty).";
    if (a.status === "unrecognised") return "The meter's logo area holds something unrecognised.";
    const mode = a.mode === "alpha" ? "transparent" : "opaque";
    return `The meter holds a ${a.width} × ${a.height} logo (${mode}).`;
  });

  /** What the running write is doing, in words. */
  const progressLabel = $derived.by(() => {
    const p = logo.progress;
    if (p === null) return "Starting the write…";
    return p.step === "chunks"
      ? `Writing chunk ${p.done} of ${p.total}`
      : `Read-back ${p.done} of ${p.total}`;
  });

  function bytes(n: number): string {
    return `${n.toLocaleString("en-US")} bytes`;
  }
</script>

<section class="logo" aria-label="Startup logo">
  <div class="pick">
    <button type="button" disabled={logo.busy} onclick={() => void logo.choose()}>
      Choose image…
    </button>
    <fieldset class="mode" disabled={logo.busy}>
      <legend>Background</legend>
      <label>
        <input
          type="radio"
          name="logo-mode"
          checked={!logo.alpha}
          onchange={() => void logo.setAlpha(false)}
        />
        Opaque
      </label>
      <label>
        <input
          type="radio"
          name="logo-mode"
          checked={logo.alpha}
          onchange={() => void logo.setAlpha(true)}
        />
        Transparent
      </label>
    </fieldset>
  </div>
  <p class="formats">PNG, JPEG, BMP or a raw logo file (.bin).</p>

  <p class="duration">
    Startup screen: <span data-testid="logo-duration">{logo.startupDuration ?? "not read yet"}</span
    >
  </p>
  {#if logo.durationOff}
    <div class="duration-hint" data-testid="logo-duration-hint">
      <p>The startup screen is off, so the logo will not show. Show it for:</p>
      <div class="duration-choices">
        {#each [1, 3, 5] as const as s (s)}
          <button
            type="button"
            disabled={logo.busy || logo.durationWriting}
            onclick={() => void logo.setDuration(s)}>{s} s</button
          >
        {/each}
      </div>
      {#if logo.durationError}
        <p class="error">{logo.durationError.message}</p>
      {/if}
    </div>
  {/if}

  {#if logo.error && !logo.preview}
    <p class="error" role="alert">{logo.error.message}</p>
  {/if}

  {#if logo.preview}
    {@const p = logo.preview}
    <div class="preview">
      <canvas
        bind:this={canvas}
        data-testid="logo-preview"
        width={p.width}
        height={p.height}
        style:width="{p.width}px"
        style:height="{p.height}px"
        aria-label="Logo preview, {p.width} × {p.height} pixels"
      ></canvas>
    </div>
    <dl class="numbers">
      <div>
        <dt>Size</dt>
        <dd data-testid="logo-size">{p.width} × {p.height}</dd>
      </div>
      <div>
        <dt>Blob</dt>
        <dd data-testid="logo-blob-size">{bytes(p.blob_size)}</dd>
      </div>
      <div>
        <dt>Chunks</dt>
        <dd data-testid="logo-chunks">{p.chunks}</dd>
      </div>
    </dl>
    {#if p.small_side_warning}
      <p class="warning" data-testid="logo-small-warning">
        A side is below 48 pixels: the logo may be hard to see on the meter.
      </p>
    {/if}
  {/if}

  <div class="writes">
    <button
      type="button"
      disabled={logo.busy || logo.uploadDisabledReason !== null}
      title={logo.uploadDisabledReason ?? undefined}
      onclick={() => logo.askUpload()}
    >
      Upload
    </button>
    <button
      type="button"
      disabled={logo.busy || logo.removeDisabledReason !== null}
      title={logo.removeDisabledReason ?? undefined}
      onclick={() => logo.askRemove()}
    >
      Remove
    </button>
    <button type="button" disabled={logo.busy || logo.reading} onclick={() => void logo.read()}
      >Read</button
    >
  </div>
  {#if areaLabel !== null}
    {@const a = logo.area}
    <div class="area">
      <p data-testid="logo-area">{areaLabel}</p>
      {#if a?.status === "logo"}
        <button
          type="button"
          disabled={logo.busy || logo.reading}
          onclick={() => void logo.read(true)}>Show stored logo</button
        >
      {/if}
      {#if a?.image && a.width !== null && a.height !== null}
        <div class="preview">
          <canvas
            bind:this={storedCanvas}
            data-testid="logo-stored"
            width={a.width}
            height={a.height}
            style:width="{a.width}px"
            style:height="{a.height}px"
            aria-label="Stored logo, {a.width} × {a.height} pixels"
          ></canvas>
        </div>
      {/if}
    </div>
  {/if}
  {#if !logo.busy && (logo.uploadDisabledReason ?? logo.removeDisabledReason) !== null}
    <p class="disabled-reason" data-testid="logo-disabled-reason">
      {logo.uploadDisabledReason ?? logo.removeDisabledReason}
    </p>
  {/if}

  {#if logo.phase === "writing"}
    <div class="progress">
      <progress
        aria-label={progressLabel}
        max={logo.progress?.total ?? 1}
        value={logo.progress?.done ?? 0}
      ></progress>
      <p aria-live="polite">{progressLabel}. This cannot be stopped; keep the meter plugged in.</p>
    </div>
  {:else if logo.phase === "done"}
    <p class="done" role="status">
      {logo.action === "remove"
        ? "Logo removed: read-back verified."
        : "Logo uploaded: read-back verified."}
    </p>
  {:else if logo.phase === "failed" && logo.failure}
    {@const f = logo.failure}
    <div class="error" role="alert">
      <p>
        {f.step} failed{f.chunk !== null ? ` at chunk ${f.chunk}` : ""}: {logo.error?.message}
      </p>
      <p>{f.advice}</p>
    </div>
  {/if}
</section>

{#if logo.confirming}
  <LogoConfirmDialog
    action={logo.confirming}
    onCancel={() => logo.cancelConfirm()}
    onConfirm={(typed) => void logo.confirm(typed)}
  />
{/if}

<style>
  .logo {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-width: 0;
  }
  .pick {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem;
  }
  .mode {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    border: none;
    margin: 0;
    padding: 0;
  }
  .mode legend {
    float: left;
    margin-right: 0.5rem;
    color: var(--muted);
  }
  .writes {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
  }
  .area {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
  }
  .area p {
    margin: 0;
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .progress progress {
    width: 100%;
    max-width: 24rem;
  }
  .progress p,
  .done {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .duration {
    margin: 0;
  }
  .duration-hint {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .duration-hint p {
    margin: 0;
  }
  .duration-choices {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .formats {
    margin: 0;
    font-size: 0.875rem;
    color: var(--muted);
  }
  .preview {
    max-width: 100%;
    overflow-x: auto;
    padding: 1px;
  }
  .preview canvas {
    display: block;
    flex: none;
    image-rendering: pixelated;
    /* An outline, not a border: the box stays the logo's exact size. */
    outline: 1px solid var(--border);
    /* A checkerboard shows through the transparent pixels. */
    background: repeating-conic-gradient(#ccc 0% 25%, #fff 0% 50%) 0 0 / 12px 12px;
  }
  .numbers {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1.5rem;
    margin: 0;
  }
  .numbers div {
    display: flex;
    gap: 0.4rem;
  }
  .numbers dt {
    color: var(--muted);
  }
  .numbers dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .warning {
    margin: 0;
    color: var(--warning, var(--text));
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .disabled-reason {
    margin: 0;
    color: var(--muted, var(--text));
  }
</style>

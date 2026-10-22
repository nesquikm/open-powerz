<script lang="ts">
  import type { PdDetails, PdField, PdRow } from "../backend";
  import type { PdCapture } from "../pdCapture.svelte";
  import PdClearDialog from "./PdClearDialog.svelte";
  import { DEFAULT_PD_FILTER, type PdFilter } from "../pdStore";
  import {
    directionLabel,
    FollowState,
    hexCells,
    rowLabel,
    timeLabel,
    visibleRange,
  } from "../pdTable";

  /**
   * The PD analyzer view (FR TGE83C): Start and Stop for the connected meter and the
   * packet table, a virtual list over the capture's record store; selecting a row shows
   * its field tree and raw bytes.
   */
  let {
    pd,
    connected,
    onStart,
    onStop,
    onClear,
    loadDetails,
  }: {
    pd: PdCapture;
    /** A meter (or the demo device) is connected and streaming. */
    connected: boolean;
    onStart: () => void;
    onStop: () => void;
    /** Empties the capture (asked after the user confirms). */
    onClear: () => void;
    /** The field tree and raw bytes of record `index`, or null when unavailable. */
    loadDetails: (index: number) => Promise<PdDetails | null>;
  } = $props();

  const controls = $derived(pd.controls(connected));

  const COLUMNS = [
    "#",
    "Time",
    "SOP",
    "Message",
    "ID",
    "Direction",
    "Objects",
    "Revision",
    "VBUS",
    "IBUS",
    "Summary",
  ] as const;

  /** Row height, px (the virtual list places rows by it). */
  const ROW_PX = 28;
  const OVERSCAN = 8;

  /** Filter state; "" in a select stands for "All". */
  let showGoodCrc = $state(DEFAULT_PD_FILTER.showGoodCrc);
  let typeChoice = $state("");
  let sopChoice = $state("");

  const filter = $derived<PdFilter>({
    showGoodCrc,
    types: typeChoice === "" ? null : new Set([typeChoice]),
    sop: sopChoice === "" ? null : sopChoice,
  });

  /** Message types and SOPs seen so far, for the filter selects. */
  const typeOptions = $derived.by(() => {
    void pd.version;
    return pd.store.messageTypes();
  });
  const sopOptions = $derived.by(() => {
    void pd.version;
    return pd.store.sops();
  });

  /** Store positions the filters let through. */
  const positions = $derived.by(() => {
    void pd.version;
    return pd.store.filter(filter);
  });

  let scrollTop = $state(0);
  let viewportPx = $state(0);

  const range = $derived(
    visibleRange({
      scrollTop,
      viewportPx,
      rowPx: ROW_PX,
      count: positions.length,
      overscan: OVERSCAN,
    }),
  );

  /** The rows to render, with their place in the filtered list. */
  const rendered = $derived.by(() => {
    void pd.version;
    const out: { at: number; row: PdRow }[] = [];
    for (let at = range.start; at < range.end; at++) {
      out.push({ at, row: pd.store.row(positions[at] as number) });
    }
    return out;
  });

  function cells(r: PdRow): string[] {
    const message = r.kind === "message";
    return [
      String(r.index + 1),
      timeLabel(r.elapsed_ms),
      r.sop ?? "",
      r.name,
      message && r.message_id !== null ? String(r.message_id) : "",
      directionLabel(r.direction),
      message && r.objects !== null ? String(r.objects) : "",
      r.spec_revision ?? "",
      `${r.vbus_v.toFixed(2)} V`,
      `${r.ibus_a.toFixed(2)} A`,
      r.summary,
    ];
  }

  /** Following new rows: on until the user scrolls up, back on with Follow. */
  const follow = new FollowState();
  let following = $state(follow.following);
  let gridEl = $state<HTMLElement | null>(null);

  function onScroll(e: Event): void {
    const el = e.currentTarget as HTMLElement;
    scrollTop = el.scrollTop;
    follow.scrolled(el);
    following = follow.following;
  }

  function scrollToEnd(): void {
    if (gridEl) gridEl.scrollTop = gridEl.scrollHeight;
  }

  function resumeFollow(): void {
    follow.resume();
    following = follow.following;
    scrollToEnd();
  }

  // New rows (or a resumed follow) keep the view at the bottom.
  $effect(() => {
    void positions.length;
    if (!following || !gridEl) return;
    // A scroll up not yet reported by a scroll event still pauses.
    follow.scrolled(gridEl);
    following = follow.following;
    if (following) scrollToEnd();
  });

  /** The selected row's record index, its details and the selected field's path. */
  let selected = $state<number | null>(null);
  let details = $state<PdDetails | null>(null);
  /** The selected row's details could not be loaded (cleared, or the meter is gone). */
  let unavailable = $state(false);
  let fieldPath = $state<string | null>(null);
  /** Paths of the expanded tree nodes. */
  let expanded = $state<Record<string, boolean>>({});
  let request = 0;

  /** Forgets the selection and drops any details still loading. */
  function resetSelection(): void {
    request++;
    selected = null;
    details = null;
    unavailable = false;
    fieldPath = null;
    expanded = {};
  }

  // A cleared table or a new session numbers rows from scratch: a kept selection
  // would point at some other row.
  let seenEmptied: number | null = null;
  $effect(() => {
    const emptied = pd.emptied;
    const first = seenEmptied === null;
    if (emptied === seenEmptied) return;
    seenEmptied = emptied;
    if (!first) resetSelection();
  });

  async function selectRow(index: number): Promise<void> {
    if (selected === index) return;
    resetSelection();
    selected = index;
    const ticket = request;
    const d = await loadDetails(index);
    if (ticket !== request) return;
    details = d;
    unavailable = d === null;
  }

  /** The filtered-list position of record `index`, or -1 (positions ascend with it). */
  function positionOf(index: number): number {
    let lo = 0;
    let hi = positions.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const at = pd.store.row(positions[mid] as number).index;
      if (at === index) return mid;
      if (at < index) lo = mid + 1;
      else hi = mid - 1;
    }
    return -1;
  }

  /** Scrolls the table so the row at filtered position `at` is in view. */
  function revealRow(at: number): void {
    if (!gridEl) return;
    const head = gridEl.querySelector<HTMLElement>(".head")?.offsetHeight ?? 0;
    const top = at * ROW_PX;
    const room = gridEl.clientHeight - head;
    let next = gridEl.scrollTop;
    if (top < next) next = top;
    else if (top + ROW_PX > next + room) next = top + ROW_PX - room;
    if (next !== gridEl.scrollTop) {
      gridEl.scrollTop = next;
      scrollTop = gridEl.scrollTop;
      follow.scrolled(gridEl);
      following = follow.following;
    }
  }

  /** Keyboard in the table: arrows, Page Up/Down, Home and End move the selection. */
  function onGridKey(e: KeyboardEvent): void {
    const count = positions.length;
    if (count === 0) return;
    const current = selected === null ? -1 : positionOf(selected);
    const page = Math.max(1, Math.floor(viewportPx / ROW_PX) - 1);
    let at: number;
    switch (e.key) {
      case "ArrowDown":
        at = current < 0 ? 0 : Math.min(count - 1, current + 1);
        break;
      case "ArrowUp":
        at = current < 0 ? 0 : Math.max(0, current - 1);
        break;
      case "PageDown":
        at = current < 0 ? 0 : Math.min(count - 1, current + page);
        break;
      case "PageUp":
        at = current < 0 ? 0 : Math.max(0, current - page);
        break;
      case "Home":
        at = 0;
        break;
      case "End":
        at = count - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    if (document.activeElement !== gridEl) gridEl?.focus({ preventScroll: true });
    revealRow(at);
    void selectRow(pd.store.row(positions[at] as number).index);
  }

  /** The selected row's id while it is rendered (for `aria-activedescendant`). */
  const activeRow = $derived(
    selected !== null && rendered.some((r) => r.row.index === selected)
      ? `pd-row-${selected}`
      : undefined,
  );

  /** Clear asks first, in an in-app alert dialog. */
  let confirmOpen = $state(false);
  let clearButton = $state<HTMLButtonElement | null>(null);

  function askClear(): void {
    confirmOpen = true;
  }

  function closeConfirm(): void {
    confirmOpen = false;
    queueMicrotask(() => clearButton?.focus());
  }

  function confirmClear(): void {
    onClear();
    resetSelection();
    closeConfirm();
  }

  /** A tree node as the keyboard walks it: path, field and depth. */
  interface TreeItem {
    path: string;
    field: PdField;
  }

  /** The tree nodes in view, in order (children of expanded nodes only). */
  const treeItems = $derived.by(() => {
    const out: TreeItem[] = [];
    const walk = (fields: PdField[], prefix: string): void => {
      fields.forEach((field, i) => {
        const path = prefix === "" ? String(i) : `${prefix}/${i}`;
        out.push({ path, field });
        if (field.children.length > 0 && expanded[path]) walk(field.children, path);
      });
    };
    walk(details?.fields ?? [], "");
    return out;
  });

  const selectedField = $derived(treeItems.find((t) => t.path === fieldPath)?.field ?? null);

  const hex = $derived(details ? hexCells(details.raw, selectedField?.bytes ?? null) : []);

  /** The tab stop of the tree: the selected node, else the first one. */
  const focusPath = $derived(fieldPath ?? treeItems[0]?.path ?? null);

  function selectField(path: string): void {
    fieldPath = path;
  }

  function parentPath(path: string): string | null {
    const cut = path.lastIndexOf("/");
    return cut < 0 ? null : path.slice(0, cut);
  }

  function focusItem(tree: HTMLElement, path: string): void {
    fieldPath = path;
    queueMicrotask(() =>
      tree.querySelector<HTMLElement>(`[data-path="${CSS.escape(path)}"]`)?.focus(),
    );
  }

  function onTreeKey(e: KeyboardEvent): void {
    e.stopPropagation();
    const tree = (e.currentTarget as HTMLElement).closest<HTMLElement>('[role="tree"]');
    if (!tree) return;
    const at = treeItems.findIndex((t) => t.path === focusPath);
    const item = treeItems[at];
    if (!item) return;
    const hasChildren = item.field.children.length > 0;
    switch (e.key) {
      case "ArrowDown": {
        const next = treeItems[at + 1];
        if (next) focusItem(tree, next.path);
        break;
      }
      case "ArrowUp": {
        const prev = treeItems[at - 1];
        if (prev) focusItem(tree, prev.path);
        break;
      }
      case "Home": {
        const first = treeItems[0];
        if (first) focusItem(tree, first.path);
        break;
      }
      case "End": {
        const last = treeItems[treeItems.length - 1];
        if (last) focusItem(tree, last.path);
        break;
      }
      case "ArrowRight":
        if (hasChildren && !expanded[item.path]) expanded[item.path] = true;
        else if (hasChildren) focusItem(tree, `${item.path}/0`);
        break;
      case "ArrowLeft": {
        const parent = parentPath(item.path);
        if (hasChildren && expanded[item.path]) expanded[item.path] = false;
        else if (parent !== null) focusItem(tree, parent);
        break;
      }
      case "Enter":
      case " ":
        fieldPath = item.path;
        break;
      default:
        return;
    }
    e.preventDefault();
  }

  function toggle(path: string, e: MouseEvent): void {
    e.stopPropagation();
    expanded[path] = !expanded[path];
    fieldPath = path;
  }
</script>

{#snippet treeNode(field: PdField, path: string, level: number)}
  {@const open = field.children.length > 0 ? !!expanded[path] : undefined}
  <div
    role="treeitem"
    class="node"
    data-path={path}
    aria-label="{field.name}: {field.meaning}"
    aria-level={level}
    aria-selected={fieldPath === path}
    aria-expanded={open}
    tabindex={focusPath === path ? 0 : -1}
    onclick={(e) => {
      e.stopPropagation();
      selectField(path);
    }}
    onkeydown={onTreeKey}
  >
    <div class="label" style:padding-left="{(level - 1) * 1}rem">
      {#if open !== undefined}
        <button
          type="button"
          class="twisty"
          tabindex="-1"
          aria-hidden="true"
          onclick={(e) => toggle(path, e)}>{open ? "▾" : "▸"}</button
        >
      {:else}
        <span class="twisty"></span>
      {/if}
      <span class="name">{field.name}</span>
      <span class="meaning">{field.meaning}</span>
    </div>
    {#if open}
      <div role="group">
        {#each field.children as child, i (i)}
          {@render treeNode(child, `${path}/${i}`, level + 1)}
        {/each}
      </div>
    {/if}
  </div>
{/snippet}

<div class="pd">
  <div class="toolbar" role="toolbar" aria-label="PD capture">
    <button
      type="button"
      class="primary"
      disabled={!controls.canStart}
      aria-describedby={controls.reason ? "pd-reason" : undefined}
      onclick={onStart}>Start capture</button
    >
    <button
      type="button"
      disabled={!controls.canStop}
      aria-describedby={controls.reason ? "pd-reason" : undefined}
      onclick={onStop}>Stop capture</button
    >
    <button type="button" bind:this={clearButton} onclick={askClear}>Clear capture</button>
    <button type="button" aria-pressed={following} onclick={resumeFollow}>Follow</button>
    {#if controls.reason}
      <p id="pd-reason" class="reason">{controls.reason}</p>
    {:else if pd.capturing}
      <p class="reason" aria-live="polite">Capturing…</p>
    {/if}
  </div>

  <div class="toolbar filters" role="group" aria-label="PD filters">
    <label class="check">
      <input type="checkbox" bind:checked={showGoodCrc} />
      Show GoodCRC
    </label>
    <label for="pd-type">Message type</label>
    <select id="pd-type" bind:value={typeChoice}>
      <option value="">All</option>
      {#each typeOptions as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </select>
    <label for="pd-sop">SOP</label>
    <select id="pd-sop" bind:value={sopChoice}>
      <option value="">All</option>
      {#each sopOptions as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </select>
  </div>

  <div
    class="grid panel"
    role="grid"
    aria-label="PD packets"
    aria-rowcount={positions.length + 1}
    aria-colcount={COLUMNS.length}
    tabindex="0"
    aria-activedescendant={activeRow}
    bind:this={gridEl}
    bind:clientHeight={viewportPx}
    onscroll={onScroll}
    onkeydown={onGridKey}
  >
    <div role="rowgroup" class="head">
      <div role="row" class="row" aria-rowindex={1}>
        {#each COLUMNS as name (name)}
          <div role="columnheader" class="cell">{name}</div>
        {/each}
      </div>
    </div>
    <div role="rowgroup" class="body" style:height="{positions.length * ROW_PX}px">
      {#each rendered as { at, row } (row.index)}
        <div
          role="row"
          id="pd-row-{row.index}"
          class="row"
          class:event={row.kind === "event"}
          data-index={row.index}
          data-kind={row.kind}
          aria-rowindex={at + 2}
          aria-selected={selected === row.index}
          tabindex="-1"
          aria-label={rowLabel(row)}
          style:top="{at * ROW_PX}px"
          onclick={() => void selectRow(row.index)}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              void selectRow(row.index);
            }
          }}
        >
          {#each cells(row) as text, c (c)}
            <div role="gridcell" class="cell">{text}</div>
          {/each}
        </div>
      {/each}
    </div>
  </div>

  <section class="details panel" aria-label="Packet details">
    {#if details}
      <div class="tree" role="tree" aria-label="Fields" tabindex="-1">
        {#each details.fields as field, i (i)}
          {@render treeNode(field, String(i), 1)}
        {/each}
      </div>
      <div class="hex" aria-label="Raw bytes" role="group">
        {#each hex as cell, i (i)}
          {#if cell.highlighted}
            <mark data-byte={i}>{cell.text}</mark>
          {:else}
            <span data-byte={i}>{cell.text}</span>
          {/if}
        {/each}
      </div>
    {:else}
      <p class="reason">
        {selected === null
          ? "Select a row to see its fields and bytes."
          : unavailable
            ? "Details unavailable."
            : "Loading…"}
      </p>
    {/if}
  </section>

  {#if confirmOpen}
    <PdClearDialog onCancel={closeConfirm} onConfirm={confirmClear} />
  {/if}
</div>

<style>
  .pd {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    height: 100%;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  .filters label {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.875rem;
  }
  .reason {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .grid {
    flex: 1;
    min-height: 16rem;
    overflow: auto;
    position: relative;
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }
  .row {
    display: grid;
    grid-template-columns: 4rem 5.5rem 3.5rem 12rem 2.5rem 8rem 5rem 4.5rem 5rem 5rem minmax(
        16rem,
        1fr
      );
    align-items: center;
    height: 28px;
    min-width: 72rem;
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
  }
  /* The row, not the group, is as wide as the columns: give it the background
     too, or rows show through the header once the grid scrolls sideways. */
  .head .row {
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    font-weight: 600;
  }
  .body {
    position: relative;
  }
  .body .row {
    position: absolute;
    left: 0;
    right: 0;
  }
  .cell {
    padding: 0 0.5rem;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .row.event {
    font-style: italic;
    background: var(--bg);
  }
  .body .row {
    cursor: pointer;
  }
  .body .row[aria-selected="true"] {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .details {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 1rem;
    height: 14rem;
    flex: none;
    overflow: hidden;
    font-size: 0.8125rem;
  }
  .details > .reason {
    grid-column: 1 / -1;
  }
  .tree,
  .hex {
    overflow: auto;
    min-height: 0;
  }
  .node:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .label {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    padding: 0.125rem 0.25rem;
    cursor: pointer;
  }
  .node[aria-selected="true"] > .label {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .twisty {
    display: inline-block;
    width: 1rem;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .name {
    font-weight: 600;
  }
  .meaning {
    color: var(--muted);
  }
  .hex {
    display: flex;
    flex-wrap: wrap;
    align-content: flex-start;
    gap: 0.25rem 0.5rem;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .hex > * {
    padding: 0 0.125rem;
    border-radius: 2px;
  }
  .hex mark {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
    color: inherit;
  }
</style>

<script lang="ts">
  import { badgeLabel } from "../format";
  import type { Session } from "../session.svelte";
  import type { ThemeChoice } from "../theme";
  import DevicePicker from "./DevicePicker.svelte";

  let {
    session,
    themeChoice,
    onCycleTheme,
    onToggleConnect,
    onAbout,
    rateSps,
  }: {
    session: Session;
    themeChoice: ThemeChoice;
    onCycleTheme: () => void;
    onToggleConnect: () => void;
    onAbout: () => void;
    /** The rate the next connect asks for, or the session's current rate. */
    rateSps: number;
  } = $props();

  const THEME_LABELS: Record<ThemeChoice, string> = {
    system: "System",
    light: "Light",
    dark: "Dark",
  };
</script>

<header class="top-bar" aria-label="Device controls">
  <div class="group">
    <span class="brand">open-powerz</span>
    <DevicePicker {session} />
    <button
      type="button"
      class:primary={!session.active}
      disabled={!session.canToggleConnect}
      onclick={onToggleConnect}
    >
      {session.active ? "Disconnect" : "Connect"}
    </button>
    <span class="badge" data-state={session.state} role="status" aria-live="polite"
      >{badgeLabel(session.state)}</span
    >
  </div>
  <div class="group end">
    <span class="rate">
      <!-- Shows the rate; picking it here arrives with recording (FR 0P7HSJ).
           Keys 1–4 already change it. -->
      <select disabled aria-label="Sample rate" value={rateSps}>
        {#each [2, 10, 50, 1000] as sps (sps)}
          <option value={sps}>{sps} SPS</option>
        {/each}
      </select>
    </span>
    <button type="button" disabled>Record</button>
    <button
      type="button"
      class="theme-toggle"
      title="Cycle theme: System, Light, Dark"
      onclick={onCycleTheme}
    >
      {THEME_LABELS[themeChoice]} theme
    </button>
    <button type="button" onclick={onAbout}>About</button>
  </div>
</header>

<style>
  .top-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1.5rem;
    min-width: 0;
    padding: 0.625rem 1rem;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  /* Each group wraps as a whole, so a narrow window gets two tidy rows. */
  .group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .brand {
    font-weight: 700;
    letter-spacing: 0.01em;
    margin-right: 0.5rem;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.15rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 0.8125rem;
    background: var(--bg);
  }
  .badge::before {
    content: "";
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--control-border);
  }
  .badge[data-state="connecting"]::before {
    background: var(--accent);
  }
  .badge[data-state="streaming"]::before {
    background: var(--chart-power);
  }
  .badge[data-state="rate_locked"]::before {
    background: var(--chart-d-minus);
  }
  .badge[data-state="error"]::before {
    background: var(--danger);
  }
</style>

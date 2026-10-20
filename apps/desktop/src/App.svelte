<script lang="ts">
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { formatVolts } from "./lib/units";

  let info = $state("loading…");

  $effect(() => {
    if (!isTauri()) {
      info = "running in a browser (no device access)";
      return;
    }
    invoke<string>("app_info").then(
      (text) => (info = text),
      (err: unknown) => (info = `error: ${String(err)}`),
    );
  });
</script>

<main>
  <h1>open-powerz</h1>
  <p class="tagline">
    Unofficial desktop app for the POWER-Z KM003C / KM002C. No device support yet.
  </p>
  <p class="reading" aria-label="VBUS">{formatVolts(0)}</p>
  <p class="info">{info}</p>
</main>

<style>
  main {
    max-width: 48rem;
    margin: 4rem auto;
    padding: 0 1rem;
  }
  .reading {
    font-size: 3rem;
    font-variant-numeric: tabular-nums;
  }
  .tagline,
  .info {
    color: var(--muted);
  }
</style>

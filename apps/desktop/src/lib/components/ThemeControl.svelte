<script lang="ts">
  import type { ThemeChoice } from "../theme";

  /** System / Light / Dark as one segmented control: the pressed segment is the current choice.
   * `inMenu`: the three are menu item radios of the surrounding menu (its parent moves focus). */
  let {
    choice,
    onChoose,
    inMenu = false,
  }: {
    choice: ThemeChoice;
    onChoose: (choice: ThemeChoice) => void;
    inMenu?: boolean;
  } = $props();

  const OPTIONS: { value: ThemeChoice; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
</script>

<div
  class="segmented"
  role={inMenu ? "group" : "radiogroup"}
  aria-label="Theme"
  title="Theme (T cycles)"
>
  {#each OPTIONS as option (option.value)}
    <button
      type="button"
      role={inMenu ? "menuitemradio" : "radio"}
      tabindex={inMenu ? -1 : undefined}
      aria-checked={choice === option.value}
      class:selected={choice === option.value}
      onclick={() => onChoose(option.value)}>{option.label}</button
    >
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    border: 1px solid var(--control-border);
    border-radius: 0.375rem;
    overflow: hidden;
  }
  .segmented button {
    border: 0;
    border-radius: 0;
    background: var(--surface);
  }
  .segmented button + button {
    border-left: 1px solid var(--control-border);
  }
  .segmented button.selected {
    color: var(--bg);
    background: var(--text);
    font-weight: 600;
  }
</style>

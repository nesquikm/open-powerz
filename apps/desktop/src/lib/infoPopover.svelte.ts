import { GLOSSARY, type GlossaryKey } from "./glossary";

/**
 * The one glossary popover open at a time (FR CW66T1, AC .5). InfoButton and Term
 * share it, keyed by their popover id, so opening one closes any other.
 */
class InfoPopover {
  /** The key of the open popover, or null when none is open. */
  openKey = $state<string | null>(null);

  open(key: string): void {
    this.openKey = key;
  }

  close(): void {
    this.openKey = null;
  }

  /** A second press of the same trigger closes it; another trigger's press swaps. */
  toggle(key: string): void {
    this.openKey = this.openKey === key ? null : key;
  }
}

export const infoPopover = new InfoPopover();

/**
 * The state InfoButton and Term share: the entry of `key`, the popover id made from the
 * component's `$props.id()`, whether it is open, and the press that toggles it.
 */
export function glossaryTrigger(key: () => GlossaryKey, uid: string) {
  const popoverId = `${uid}-popover`;
  const entry = $derived(GLOSSARY[key()]);
  const open = $derived(infoPopover.openKey === popoverId);
  return {
    popoverId,
    get entry() {
      return entry;
    },
    get open() {
      return open;
    },
    toggle: () => infoPopover.toggle(popoverId),
  };
}

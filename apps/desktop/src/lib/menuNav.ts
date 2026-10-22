/**
 * Roving focus in a menu (the WAI-ARIA menu pattern): the index of the item that
 * `key` moves focus to from `index` (-1 when no item has focus) among `count`
 * items, or null when the key does not move focus. Arrows wrap; Home and End go
 * to the first and last item.
 */
export function menuIndexAfter(key: string, index: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case "ArrowDown":
      return index < 0 ? 0 : (index + 1) % count;
    case "ArrowUp":
      return index < 0 ? count - 1 : (index - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}

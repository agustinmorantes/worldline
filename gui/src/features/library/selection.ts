// Picking several games in the sidebar: Ctrl (or ⌘) adds or takes out one, Shift takes the run
// from the last one picked, in the order the list shows.

export interface Selection {
  /** Picked games, in the list's order. */
  ids: number[];
  /** Where a Shift run starts: the last game picked with Ctrl, or the one on screen. */
  anchor: number | null;
}

export const noSelection: Selection = { ids: [], anchor: null };

export interface Modifiers {
  toggle: boolean;
  range: boolean;
}

/** What a click with these keys held does to the selection; `null` for a plain click. */
export function selectionModifiers(event: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }): Modifiers | null {
  const toggle = event.ctrlKey || event.metaKey;
  return toggle || event.shiftKey ? { toggle, range: event.shiftKey } : null;
}

/**
 * The selection after picking `id` with `mods`, among the games shown in `order`. The game on
 * screen (`current`) counts as picked when nothing else is, as it is the one highlighted.
 */
export function select(selection: Selection, id: number, order: number[], mods: Modifiers, current: number | null): Selection {
  const fresh = selection.ids.length === 0 && current !== null && order.includes(current);
  const picked = new Set(fresh ? [current] : selection.ids);
  const anchor = fresh ? current : selection.anchor;
  const from = anchor === null ? -1 : order.indexOf(anchor);
  const to = order.indexOf(id);
  if (mods.range && from !== -1 && to !== -1) {
    const run = order.slice(Math.min(from, to), Math.max(from, to) + 1);
    // Ctrl+Shift adds the run; Shift alone replaces what was picked with it.
    const ids = new Set(mods.toggle ? [...picked, ...run] : run);
    return { ids: order.filter((i) => ids.has(i)), anchor };
  }
  if (picked.has(id)) picked.delete(id);
  else picked.add(id);
  return { ids: order.filter((i) => picked.has(i)), anchor: id };
}

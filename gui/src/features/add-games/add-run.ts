// Adding the scan's games, one at a time. The run lives here rather than in the view, so it
// keeps going when the user leaves the page, shows its progress when they come back, and a
// second "Add" can't start another run over the same games.
import { useSyncExternalStore } from 'react';
import { toast } from 'sonner';
import type { Game } from '@/ipc/bindings/Game';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { saveGame, setGameIcon } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { getT } from '@/shared/i18n';
import { addFailedHint } from './add-games';

export interface AddRun {
  total: number;
  /** The game being added now; `null` once the run is over. */
  current: string | null;
  added: ReadonlySet<string>;
  failed: ReadonlySet<string>;
  running: boolean;
}

let run: AddRun | null = null;
const listeners = new Set<() => void>();
const addedListeners = new Set<(title: string, game: Game) => void>();

function update(next: AddRun): void {
  run = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** The latest run, finished or not; `null` before the first. */
export function useAddRun(): AddRun | null {
  return useSyncExternalStore(subscribe, () => run);
}

/** Calls `listener` as each game lands in the library, e.g. to fly its icon there. */
export function onGameAdded(listener: (title: string, game: Game) => void): () => void {
  addedListeners.add(listener);
  return () => { addedListeners.delete(listener); };
}

/** Adds `items` in order. A no-op while another run is going. */
export async function addAll(items: InstallScannerItem[]): Promise<void> {
  if (run?.running === true || items.length === 0) return;
  let state: AddRun = { total: items.length, current: items[0].title, added: new Set(), failed: new Set(), running: true };
  update(state);
  for (const item of items) {
    update(state = { ...state, current: item.title });
    try {
      const game = await saveGame({ id: null, title: item.title }, item.dirs);
      if (item.icon_path !== null) {
        // The game is added either way; it just keeps its initials.
        game.icon_path = await setGameIcon(game.id, item.icon_path).catch(() => null);
      }
      update(state = { ...state, added: new Set(state.added).add(item.title) });
      await queryClient.invalidateQueries({ queryKey: ['games'] });
      for (const listener of addedListeners) listener(item.title, game);
    } catch (err) {
      update(state = { ...state, failed: new Set(state.failed).add(item.title) });
      const t = getT();
      reportError(t.addGames.failed.addNamed(item.title), err, addFailedHint(err, t.addGames.why));
    }
  }
  update(state = { ...state, current: null, running: false });
  if (state.added.size > 0) toast.success(getT().addGames.addedCount(state.added.size));
}

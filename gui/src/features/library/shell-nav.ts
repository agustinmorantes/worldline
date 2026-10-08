// Navigation between the sidebar entries. The shell (`app/shell/Shell.tsx`) folds the pane
// into the old entry's icon and unfolds it from the new one; the sidebar and the game header
// read this context to hide the icon that is in flight.
import { createContext, use } from 'react';
import type { FullGame } from '@/ipc/bindings/FullGame';

export type NavTarget = { view: 'game'; gameId: number } | { view: 'add' } | { view: 'settings' };

/** What a flying icon needs from its game. */
export type FlyingGame = Pick<FullGame, 'id' | 'title' | 'accent_hue' | 'icon_path'>;

/** `data-nav-anchor` value of the sidebar icon for a target. */
export function anchorKey(target: NavTarget): string {
  return target.view === 'game' ? `game:${target.gameId.toString()}` : target.view;
}

export interface ShellNav {
  /** Where the user is. */
  current: NavTarget | null;
  /** Set while the pane folds and unfolds. */
  pending: NavTarget | null;
  go: (target: NavTarget) => void;
  /** Flies a just-added game's icon from `from` into its new sidebar row. */
  land: (game: FlyingGame, from: DOMRect) => void;
  /**
   * Games whose icon is flying: `home` back to its sidebar row, `out` to the game header,
   * `landing` into the sidebar after being added.
   */
  flying: { home: number | null; out: number | null; landing: number[] };
}

export const ShellNavContext = createContext<ShellNav>({
  current: null,
  pending: null,
  go: () => undefined,
  land: () => undefined,
  flying: { home: null, out: null, landing: [] },
});
ShellNavContext.displayName = 'ShellNav';

export function useShellNav(): ShellNav {
  return use(ShellNavContext);
}

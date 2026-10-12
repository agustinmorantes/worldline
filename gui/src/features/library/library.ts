// Pure logic behind the sidebar library.
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { GameSyncStatus } from '@/ipc/bindings/GameSyncStatus';
import { utcDate } from '@/ipc/time';

/** Up to two letters for the icon placeholder: "Elden Ring" → "ER". */
export function initials(title: string): string {
  return title
    .split(/\s+/)
    .filter((word) => word !== '')
    .map((word) => word.charAt(0))
    .join('')
    .slice(0, 2)
    .toUpperCase();
}

/** A stable hue for things that are not games yet (scan results). Games carry their own. */
export function titleHue(title: string): number {
  let hash = 0;
  for (const char of title) hash = (hash * 31 + char.charCodeAt(0)) % 360;
  return hash;
}

/** When the game was last played, across all playthroughs. */
export function lastPlayed(game: FullGame): Date | null {
  return game.last_played_at === null ? null : utcDate(game.last_played_at);
}

/** Most recently played first; games without known activity last, by title. */
export function sortLibrary(games: FullGame[]): FullGame[] {
  return [...games].sort((a, b) => {
    const at = lastPlayed(a)?.getTime() ?? null;
    const bt = lastPlayed(b)?.getTime() ?? null;
    if (at === null && bt === null) return a.title.localeCompare(b.title);
    if (at === null) return 1;
    if (bt === null) return -1;
    return bt - at;
  });
}

/** Whether `title` contains `query`, ignoring case and accents: "pokemon" finds "Pokémon". */
export function matchesTitle(title: string, query: string): boolean {
  const plain = (text: string): string => text.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();
  return plain(title).includes(plain(query.trim()));
}

export type GameStatus =
  | { kind: 'recording' }
  | { kind: 'new-from'; device: string }
  | { kind: 'played-too'; device: string }
  | { kind: 'not-in-cloud' }
  | { kind: 'no-saves' }
  | { kind: 'saved' }
  | { kind: 'last-played'; at: Date };

export interface GameSyncView {
  /** A cloud folder is set up. */
  on: boolean;
  /** A sync report has arrived. */
  reported: boolean;
  /** The game's entry in that report; a game missing from it was never sent. */
  status: GameSyncStatus | undefined;
}

/** The row's status line. A running game says so above anything else. */
export function gameStatus(game: FullGame, sync: GameSyncView, running = false): GameStatus {
  if (running) return { kind: 'recording' };
  const device = sync.status?.remote_device ?? null;
  if (sync.status?.relation === 'fast_forward' && device !== null) return { kind: 'new-from', device };
  if (sync.status?.relation === 'diverged' && device !== null) return { kind: 'played-too', device };
  const at = lastPlayed(game);
  const hasSaves = game.versions.length > 0 || game.slot_summaries.some((slot) => slot.version_count > 0);
  if (at === null && !hasSaves) return { kind: 'no-saves' };
  if (sync.on && sync.reported && sync.status === undefined) return { kind: 'not-in-cloud' };
  if (at === null) return { kind: 'saved' };
  return { kind: 'last-played', at };
}

/** The running game ids after `game-started` (`running`) or `game-stopped` for `gameId`. */
export function withRunning(ids: number[] | undefined, gameId: number, running: boolean): number[] {
  const rest = (ids ?? []).filter((id) => id !== gameId);
  return running ? [...rest, gameId] : rest;
}

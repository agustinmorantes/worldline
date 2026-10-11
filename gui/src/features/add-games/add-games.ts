// Pure logic behind the Add games view.
import type { Messages } from '@/shared/i18n';
import type { BundleTrackedDir } from '@/ipc/bindings/BundleTrackedDir';
import type { ErrorKind } from '@/ipc/bindings/ErrorKind';
import type { Failure } from '@/ipc/bindings/Failure';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { HistoryBundleSummary } from '@/ipc/bindings/HistoryBundleSummary';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';

const normalize = (title: string): string => title.trim().replace(/\s+/g, ' ').toLocaleLowerCase();

/** Keep the first 90 ms beat, then accelerate so even a large scan lands within 1.2 s. */
export function scanRevealDelay(index: number): number {
  return 1.2 * (1 - Math.exp(-index / (1.2 / 0.09)));
}

/**
 * Scan results worth offering: not in the library, not waiting in the cloud. Most recently
 * saved first; games without a save time last, by title.
 */
export function newScanResults(items: InstallScannerItem[], library: FullGame[], cloudTitles: string[]): InstallScannerItem[] {
  const known = new Set([...library.map((g) => normalize(g.title)), ...cloudTitles.map(normalize)]);
  return items
    .filter((item) => !known.has(normalize(item.title)))
    .sort((a, b) => {
      if (a.last_modified_at === null && b.last_modified_at === null) return a.title.localeCompare(b.title);
      if (a.last_modified_at === null) return 1;
      if (b.last_modified_at === null) return -1;
      return b.last_modified_at - a.last_modified_at;
    });
}

/** The folder's own name: the default title for a game added from it. */
export function folderName(path: string): string {
  return path.split(/[\\/]/).filter((part) => part !== '' && !part.endsWith(':')).pop() ?? '';
}

/** The bundle's save folders that need a place on this PC: the ones the local game lacks. */
export function dirsToMap(summary: HistoryBundleSummary, library: FullGame[]): BundleTrackedDir[] {
  const local = library.find((g) => g.uuid === summary.game_uuid);
  const tracked = new Set(local?.tracked_dirs.map((d) => d.uuid));
  return summary.active_tracked_dirs.filter((dir) => !tracked.has(dir.uuid));
}

/** Why a game couldn't be added, when core tells the reason apart; the rest only have details. */
export function addFailedHint(err: unknown, why: Messages['addGames']['why']): string | undefined {
  const kind = typeof err === 'object' && err !== null ? (err as Partial<Failure>).kind : undefined;
  const hints: Partial<Record<ErrorKind, string>> = {
    name_taken: why.nameTaken,
    folder_taken: why.folderTaken,
    shared_folder: why.sharedFolder,
    too_large: why.tooLarge,
    saves_in_use: why.savesInUse,
    game_running: why.savesInUse,
  };
  return kind === undefined || kind === null ? undefined : hints[kind];
}

/** Whether the add failed only because the saves are far larger than usual: the user may insist. */
export function isTooLarge(err: unknown): boolean {
  return typeof err === 'object' && err !== null && (err as Partial<Failure>).kind === 'too_large';
}

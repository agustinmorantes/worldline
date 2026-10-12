// Several games picked in the sidebar: the right-click menu's plural items, acting on all of them
// with one call each way, and the confirmation that lists them before deleting.
import { type ReactElement, useState } from 'react';
import { useRouter } from '@tanstack/react-router';
import { Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { GameResult } from '@/ipc/bindings/GameResult';
import type { RemoteGameSummary } from '@/ipc/bindings/RemoteGameSummary';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import { deleteGames, recoverGames, untrackGames } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { useShellNav } from '@/features/library/shell-nav';
import { GameIcon } from '@/features/library/GameIcon';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { btn, btnQuiet, row } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';
import type { MenuParts } from './GameMenu';

const listOf = (titles: string[]): string => titles.join(', ');

const outcome = (result: GameResult): GameResult['outcome']['outcome'] => result.outcome.outcome;

/** The failures among `results`, for the log and "Copy details". */
const failures = (results: GameResult[]): unknown[] => results.flatMap((r) => (r.outcome.outcome === 'failed' ? [{ title: r.title, ...r.outcome.failure }] : []));

const invalidate = (): void => {
  void queryClient.invalidateQueries({ queryKey: ['games'] });
  void queryClient.invalidateQueries({ queryKey: ['sync-report'] });
  void queryClient.invalidateQueries({ queryKey: ['recently-deleted'] });
};

/**
 * The plural items for `games`, and the dialog they open. `inCloud` says which ones can leave this
 * PC. `onSettled` gets the games that left (none for "Clear selection"), once per action.
 */
export function useBulkMenu(
  games: FullGame[],
  inCloud: (game: FullGame) => boolean,
  onSettled: (left: number[]) => void,
): { items: (Menu: MenuParts) => ReactElement; dialogs: ReactElement } {
  const t = useT();
  const router = useRouter();
  const { current } = useShellNav();
  const [confirming, setConfirming] = useState(false);
  // Kept while the dialog animates out, so its list doesn't empty first.
  const [toDelete, setToDelete] = useState<FullGame[]>([]);
  const [busy, setBusy] = useState(false);
  const eligible = games.filter(inCloud);
  const skipped = games.filter((game) => !inCloud(game)).map((game) => game.title);

  /** The games in `results` that are gone from this PC: leave the screen if it showed one. */
  function settle(left: GameResult[], cloud: (games: RemoteGameSummary[]) => RemoteGameSummary[]): void {
    const ids = left.flatMap((r) => (r.game_id === null ? [] : [r.game_id]));
    if (current?.view === 'game' && ids.includes(current.gameId)) void router.navigate({ to: '/' });
    onSettled(ids);
    // The sync that would update the cloud list can take a while.
    queryClient.setQueryData<SyncReport>(['sync-report'], (old) => old && { ...old, remote_games: cloud(old.remote_games) });
    invalidate();
  }

  async function removeFromPc(targets: FullGame[], stay: string[]): Promise<void> {
    let results: GameResult[];
    try {
      results = await untrackGames(targets.map((game) => game.id));
    } catch (err) {
      reportError(t.game.failed.remove, err, t.game.tryAgainSoon);
      return;
    }
    const removed = results.filter((r) => outcome(r) === 'done');
    const failed = results.filter((r) => outcome(r) === 'failed');
    const stayed = [...stay, ...results.filter((r) => outcome(r) === 'not_in_cloud').map((r) => r.title)];
    const uuids = new Set(removed.map((r) => r.uuid));
    settle(removed, (cloud) => cloud.map((g) => (uuids.has(g.uuid) ? { ...g, adopted: false } : g)));
    const skippedNote = stayed.length > 0 ? t.bulk.skipped(listOf(stayed), stayed.length) : undefined;
    if (removed.length > 0) toast.success(t.bulk.removed(removed.length), { description: skippedNote });
    else if (skippedNote !== undefined) toast.info(skippedNote);
    if (failed.length > 0) {
      const again = targets.filter((game) => failed.some((r) => r.game_id === game.id));
      reportError(t.bulk.failed.remove(listOf(failed.map((r) => r.title))), failures(failed), t.game.tryAgainSoon, {
        label: t.errors.retry,
        onClick: () => { void removeFromPc(again, []); },
      });
    }
  }

  /** Deletes `targets` everywhere, with one undo for all of them. False when nothing was asked of the core. */
  async function deleteEverywhere(targets: FullGame[]): Promise<boolean> {
    let results: GameResult[];
    setBusy(true);
    try {
      results = await deleteGames(targets.map((game) => game.id));
    } catch (err) {
      reportError(t.game.failed.delete, err, t.game.tryAgainSoon);
      return false;
    } finally {
      setBusy(false);
    }
    setConfirming(false);
    const deleted = results.filter((r) => outcome(r) === 'done');
    const failed = results.filter((r) => outcome(r) === 'failed');
    const uuids = new Set(deleted.map((r) => r.uuid));
    settle(deleted, (cloud) => cloud.filter((g) => !uuids.has(g.uuid)));
    // Longer than the usual toast, so there is time to reach Undo.
    if (deleted.length > 0) {
      toast.success(t.bulk.deleted(deleted.length), { duration: 10_000, action: { label: t.game.undo, onClick: () => { void undoDelete(deleted); } } });
    }
    if (failed.length > 0) {
      const again = targets.filter((game) => failed.some((r) => r.game_id === game.id));
      reportError(t.bulk.failed.delete(listOf(failed.map((r) => r.title))), failures(failed), t.game.tryAgainSoon, {
        label: t.errors.retry,
        onClick: () => { void deleteEverywhere(again); },
      });
    }
    return true;
  }

  /** Brings back what `deleted` names, in one call; a retry asks only for what didn't come back. */
  async function undoDelete(deleted: GameResult[]): Promise<void> {
    const uuids = deleted.flatMap((r) => (r.uuid === null ? [] : [r.uuid]));
    try {
      const results = await recoverGames(uuids);
      const failed = results.filter((r) => outcome(r) === 'failed');
      if (failed.length > 0) {
        // Still deleted, so still in Recently deleted; trying again here is as good.
        reportError(t.bulk.failed.undo(listOf(failed.map((r) => r.title))), failures(failed), t.bulk.undoHint, {
          label: t.errors.retry,
          onClick: () => { void undoDelete(failed); },
        });
      }
    } catch (err) {
      reportError(t.game.failed.undo, err, t.bulk.undoHint, { label: t.errors.retry, onClick: () => { void undoDelete(deleted); } });
    }
    invalidate();
  }

  const items = (Menu: MenuParts): ReactElement => (
    <>
      <Menu.Label className="px-2.5 pb-1 pt-1.5 text-xs font-semibold text-tx3">{t.bulk.selected(games.length)}</Menu.Label>
      <Menu.Item
        className={cn(row, 'flex-col items-start gap-0.5')}
        disabled={eligible.length === 0}
        onSelect={() => { void removeFromPc(eligible, skipped); }}
      >
        {eligible.length === 0 ? t.game.removeFromPc : t.bulk.removeFromPc(eligible.length)}
        {skipped.length > 0 && <span className="text-xs font-medium text-tx3">{t.bulk.skipped(listOf(skipped), skipped.length)}</span>}
      </Menu.Item>
      <Menu.Item className={cn(row, 'text-err')} onSelect={() => { setToDelete(games); setConfirming(true); }}>
        {t.bulk.deleteEverywhere(games.length)}
      </Menu.Item>
      <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />
      <Menu.Item className={row} onSelect={() => { onSettled([]); }}>{t.bulk.clear}</Menu.Item>
    </>
  );

  const dialogs = (
    <Dialog open={confirming} onOpenChange={(o) => { if (!o) setConfirming(false); }}>
      <DialogContent className="bg-sf sm:max-w-[420px]">
        <DialogHeader>
          <DialogTitle className="text-lg font-bold tracking-tight">{t.bulk.deleteTitle(toDelete.length)}</DialogTitle>
          <DialogDescription className="text-tx2">{t.bulk.deleteBody}</DialogDescription>
        </DialogHeader>
        <ul className="flex max-h-[220px] flex-col gap-1 overflow-y-auto rounded-xl bg-el p-1.5">
          {toDelete.map((game) => (
            <li key={game.id} className="flex min-w-0 items-center gap-2.5 px-1.5 py-1 text-sm font-semibold">
              <GameIcon title={game.title} hue={game.accent_hue} iconPath={game.icon_path ?? undefined} iconRevision={game.icon_sha256 ?? undefined} size={24} />
              <span className="truncate">{game.title}</span>
            </li>
          ))}
        </ul>
        <DialogFooter className="items-center gap-2">
          <button type="button" className={btnQuiet} onClick={() => { setConfirming(false); }}>{t.game.cancel}</button>
          <button type="button" className={cn(btn, 'bg-err text-bg')} disabled={busy} onClick={() => { void deleteEverywhere(toDelete); }}>
            <Trash2 size={15} strokeWidth={2.4} aria-hidden="true" />
            {t.bulk.delete(toDelete.length)}
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );

  return { items, dialogs };
}

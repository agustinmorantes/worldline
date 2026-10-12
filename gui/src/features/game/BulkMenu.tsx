// Several games picked in the sidebar: the right-click menu's plural items, acting on all of them
// with one call each way, and the confirmation that lists them before deleting.
import { type ReactElement, useRef, useState } from 'react';
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
import { GameIcon } from '@/features/library/GameIcon';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { btn, btnQuiet, row } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';
import type { MenuParts } from './GameMenu';

const listOf = (titles: string[]): string => titles.join(', ');

/** A game an action was asked about, kept by uuid so a retry can't reach another game reusing its id. */
interface Target {
  uuid: string;
  title: string;
}

const targetOf = ({ uuid, title }: { uuid: string; title: string }): Target => ({ uuid, title });

type Outcome = GameResult['outcome']['outcome'];

/** `results` with each one's title, falling back to the one asked for when the core had none. */
const named = (results: GameResult[], targets: Target[]): GameResult[] =>
  results.map((r) => (r.title !== '' ? r : { ...r, title: targets.find((game) => game.uuid === r.uuid)?.title ?? '' }));

const having = (results: GameResult[], ...outcomes: Outcome[]): GameResult[] => results.filter((r) => outcomes.includes(r.outcome.outcome));

/** The failures among `results`, for the log and "Copy details". */
const failures = (results: GameResult[]): unknown[] => results.flatMap((r) => (r.outcome.outcome === 'failed' ? [{ title: r.title, ...r.outcome.failure }] : []));

/** Done, but the library is still catching up; it does on its own, so this only goes to the log. */
const logApplied = (results: GameResult[]): void => {
  for (const r of results) if (r.outcome.outcome === 'applied') console.warn(`${r.title}: ${r.outcome.warning.message}`);
};

const invalidate = (): Promise<unknown> =>
  Promise.all([
    queryClient.invalidateQueries({ queryKey: ['games'] }),
    queryClient.invalidateQueries({ queryKey: ['sync-report'] }),
    queryClient.invalidateQueries({ queryKey: ['recently-deleted'] }),
  ]);

/**
 * The plural items for `games`, and the dialog they open. `inCloud` says which ones can leave this
 * PC; `open` is the uuid of the game on screen, if any. `onSettled` gets the uuids of the games that left (none for "Clear selection"), once per
 * action. `focus` puts focus back in the library once the menu or dialog that had it is gone: on
 * the first of `prefer` still listed, else on the row after the first of `avoid`.
 */
export function useBulkMenu(
  games: FullGame[],
  inCloud: (game: FullGame) => boolean,
  open: string | null,
  onSettled: (left: string[]) => void,
  focus: (avoid: string[], prefer: string[]) => void,
): { items: (Menu: MenuParts) => ReactElement; dialogs: ReactElement } {
  const t = useT();
  const router = useRouter();
  // Read when an action ends, which may be a retry from a toast long after this render.
  const openRef = useRef(open);
  openRef.current = open;
  const [confirming, setConfirming] = useState(false);
  // Kept while the dialog animates out, so its list doesn't empty first.
  const [toDelete, setToDelete] = useState<FullGame[]>([]);
  const [busy, setBusy] = useState(false);
  // Where focus goes once the dialog closes: back on the picked games, unless they left.
  const afterDialogRef = useRef<{ avoid: string[]; prefer: string[] }>({ avoid: [], prefer: [] });
  const eligible = games.filter(inCloud);
  const skipped = games.filter((game) => !inCloud(game)).map((game) => game.title);

  /** The games `left` names are gone from this PC: leave the screen if it showed one. */
  function settle(left: GameResult[], cloud: (games: RemoteGameSummary[]) => RemoteGameSummary[]): void {
    const uuids = left.map((r) => r.uuid);
    if (openRef.current !== null && uuids.includes(openRef.current)) void router.navigate({ to: '/' });
    onSettled(uuids);
    // The sync that would update the cloud list can take a while.
    queryClient.setQueryData<SyncReport>(['sync-report'], (old) => old && { ...old, remote_games: cloud(old.remote_games) });
    void invalidate();
  }

  /** A note for games done whose library entry is still catching up. */
  const catchingUp = (results: GameResult[]): string | undefined => (having(results, 'applied').length > 0 ? t.bulk.catchingUp : undefined);

  /** Tells about games the core no longer had; there is nothing to retry. */
  function noteMissing(results: GameResult[]): void {
    const missing = having(results, 'not_found');
    if (missing.length > 0) toast.info(t.bulk.missing(listOf(missing.map((r) => r.title)), missing.length));
  }

  async function removeFromPc(targets: Target[], stay: string[]): Promise<void> {
    let results: GameResult[];
    try {
      results = named(await untrackGames(targets.map((game) => game.uuid)), targets);
    } catch (err) {
      reportError(t.game.failed.remove, err, t.game.tryAgainSoon);
      focus([], targets.map((game) => game.uuid));
      return;
    }
    const removed = having(results, 'done', 'already_done', 'applied');
    const failed = having(results, 'failed');
    const stayed = [...stay, ...having(results, 'not_in_cloud').map((r) => r.title)];
    const uuids = new Set(removed.map((r) => r.uuid));
    settle(removed, (cloud) => cloud.map((g) => (uuids.has(g.uuid) ? { ...g, adopted: false } : g)));
    focus([...uuids], failed.map((r) => r.uuid));
    logApplied(results);
    const skippedNote = stayed.length > 0 ? t.bulk.skipped(listOf(stayed), stayed.length) : undefined;
    const note = [skippedNote, catchingUp(results)].filter((line) => line !== undefined).join(' ') || undefined;
    if (removed.length > 0) toast.success(t.bulk.removed(removed.length), { description: note });
    else if (skippedNote !== undefined) toast.info(skippedNote);
    noteMissing(results);
    if (failed.length > 0) {
      const again = targets.filter((game) => failed.some((r) => r.uuid === game.uuid));
      reportError(t.bulk.failed.remove(listOf(failed.map((r) => r.title))), failures(failed), t.game.tryAgainSoon, {
        label: t.errors.retry,
        onClick: () => { void removeFromPc(again, []); },
      });
    }
  }

  /** Deletes `targets` everywhere, with one undo for all of them. False when nothing was asked of the core. */
  async function deleteEverywhere(targets: Target[], fromDialog = false): Promise<boolean> {
    let results: GameResult[];
    setBusy(true);
    try {
      results = named(await deleteGames(targets.map((game) => game.uuid)), targets);
    } catch (err) {
      reportError(t.game.failed.delete, err, t.game.tryAgainSoon);
      return false;
    } finally {
      setBusy(false);
    }
    // Deleted now or by an earlier try: the undo brings either back.
    const deleted = having(results, 'done', 'already_done', 'applied');
    const failed = having(results, 'failed');
    const uuids = new Set(deleted.map((r) => r.uuid));
    afterDialogRef.current = { avoid: [...uuids], prefer: failed.map((r) => r.uuid) };
    // A retry from a toast has no dialog to close.
    if (fromDialog) setConfirming(false);
    else focus(afterDialogRef.current.avoid, afterDialogRef.current.prefer);
    settle(deleted, (cloud) => cloud.filter((g) => !uuids.has(g.uuid)));
    logApplied(results);
    // Longer than the usual toast, so there is time to reach Undo.
    if (deleted.length > 0) {
      const undo = deleted.map(targetOf);
      toast.success(t.bulk.deleted(deleted.length), { description: catchingUp(results), duration: 10_000, action: { label: t.game.undo, onClick: () => { void undoDelete(undo); } } });
    }
    noteMissing(results);
    if (failed.length > 0) {
      const again = targets.filter((game) => failed.some((r) => r.uuid === game.uuid));
      reportError(t.bulk.failed.delete(listOf(failed.map((r) => r.title))), failures(failed), t.game.tryAgainSoon, {
        label: t.errors.retry,
        onClick: () => { void deleteEverywhere(again); },
      });
    }
    return true;
  }

  /** Brings back `deleted` in one call; a retry asks only for what didn't come back. */
  async function undoDelete(deleted: Target[]): Promise<void> {
    let results: GameResult[];
    try {
      results = named(await recoverGames(deleted.map((game) => game.uuid)), deleted);
    } catch (err) {
      reportError(t.game.failed.undo, err, t.bulk.undoHint, { label: t.errors.retry, onClick: () => { void undoDelete(deleted); } });
      focus([], []);
      return;
    }
    // The rows come back first, so focus can land on them.
    await invalidate();
    const failed = having(results, 'failed');
    focus([], having(results, 'done', 'already_done', 'applied').map((r) => r.uuid));
    logApplied(results);
    noteMissing(results);
    if (failed.length > 0) {
      // Still deleted, so still in Recently deleted; trying again here is as good.
      const again = deleted.filter((game) => failed.some((r) => r.uuid === game.uuid));
      reportError(t.bulk.failed.undo(listOf(failed.map((r) => r.title))), failures(failed), t.bulk.undoHint, {
        label: t.errors.retry,
        onClick: () => { void undoDelete(again); },
      });
    }
  }

  const items = (Menu: MenuParts): ReactElement => (
    <>
      <Menu.Label className="px-2.5 pb-1 pt-1.5 text-xs font-semibold text-tx3">{t.bulk.selected(games.length)}</Menu.Label>
      <Menu.Item
        className={cn(row, 'flex-col items-start gap-0.5')}
        disabled={eligible.length === 0}
        onSelect={() => { void removeFromPc(eligible.map(targetOf), skipped); }}
      >
        {eligible.length === 0 ? t.game.removeFromPc : t.bulk.removeFromPc(eligible.length)}
        {skipped.length > 0 && <span className="text-xs font-medium text-tx3">{t.bulk.skipped(listOf(skipped), skipped.length)}</span>}
      </Menu.Item>
      <Menu.Item
        className={cn(row, 'text-err')}
        onSelect={() => {
          setToDelete(games);
          afterDialogRef.current = { avoid: [], prefer: games.map((game) => game.uuid) };
          setConfirming(true);
        }}
      >
        {t.bulk.deleteEverywhere(games.length)}
      </Menu.Item>
      <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />
      <Menu.Item className={row} onSelect={() => { onSettled([]); }}>{t.bulk.clear}</Menu.Item>
    </>
  );

  const dialogs = (
    <Dialog open={confirming} onOpenChange={(o) => { if (!o) setConfirming(false); }}>
      <DialogContent
        className="bg-sf sm:max-w-[420px]"
        // The menu that opened it is gone, and so may be the rows it acted on.
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          focus(afterDialogRef.current.avoid, afterDialogRef.current.prefer);
        }}
      >
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
          <button type="button" className={cn(btn, 'bg-err text-bg')} disabled={busy} onClick={() => { void deleteEverywhere(toDelete.map(targetOf), true); }}>
            <Trash2 size={15} strokeWidth={2.4} aria-hidden="true" />
            {t.bulk.delete(toDelete.length)}
          </button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );

  return { items, dialogs };
}

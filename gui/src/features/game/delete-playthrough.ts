// Deleting a playthrough everywhere, with an undo in the toast: it waits in Recently deleted.
import { toast } from 'sonner';
import type { Failure } from '@/ipc/bindings/Failure';
import type { Slot } from '@/ipc/bindings/Slot';
import { deleteSlot, recoverSlot } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { getT } from '@/shared/i18n';
import { slotName } from './game-view';

const refresh = (gameId: number): void => {
  void queryClient.invalidateQueries({ queryKey: ['game', gameId] });
  void queryClient.invalidateQueries({ queryKey: ['games'] });
  void queryClient.invalidateQueries({ queryKey: ['recently-deleted'] });
};

/**
 * Deletes `slot` of `game`. The current playthrough needs `replacement`: the core switches the
 * saves to it first, behind the running-game gate and the all-or-nothing write, and deletes nothing
 * when that fails. False when it failed; the error is reported.
 */
export async function deletePlaythrough(
  game: { id: number; title: string },
  slot: Slot,
  replacement: number | undefined,
  gameRunning: boolean,
): Promise<boolean> {
  const t = getT();
  try {
    await deleteSlot(slot.id, replacement);
  } catch (err) {
    const kind = typeof err === 'object' && err !== null ? (err as Partial<Failure>).kind : undefined;
    const hint = kind === 'restore_incomplete' ? t.game.puttingBack
      : replacement !== undefined && (gameRunning || kind === 'game_running' || kind === 'saves_in_use') ? t.game.closeAndRetry(game.title)
      : t.game.tryAgainSoon;
    reportError(t.game.failed.deleteSlot, err, hint);
    refresh(game.id);
    return false;
  }
  refresh(game.id);
  // Longer than the usual toast, so there is time to reach Undo.
  toast.success(t.game.slotDeleted(slotName(slot.name, t.game.playthroughName(1))), {
    duration: 10_000,
    action: { label: t.game.undo, onClick: () => { void undoDelete(game.id, slot.uuid); } },
  });
  return true;
}

/** Brings the playthrough back, not current: the saves stay as they are. */
async function undoDelete(gameId: number, uuid: string): Promise<void> {
  const t = getT();
  try {
    await recoverSlot(uuid);
  } catch (err) {
    // Still deleted, so still in Recently deleted; trying again here is as good.
    reportError(t.game.failed.undoSlot, err, t.game.undoHint, { label: t.errors.retry, onClick: () => { void undoDelete(gameId, uuid); } });
  }
  refresh(gameId);
}

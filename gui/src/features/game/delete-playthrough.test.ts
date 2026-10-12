import { beforeEach, describe, expect, it, vi } from 'vitest';
import { toast } from 'sonner';
import { makeSlot } from '@/ipc/builders';
import { deleteSlot, recoverSlot, switchSlot } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';
import { deletePlaythrough } from './delete-playthrough';

vi.mock('@/ipc/commands', () => ({ deleteSlot: vi.fn(), recoverSlot: vi.fn(), switchSlot: vi.fn() }));
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));

const game = { id: 1, title: 'Hollow Knight' };
const slot = makeSlot({ id: 1, name: 'Main run', uuid: 'main-run' });

/** The Undo in the success toast. */
function undo(): () => void {
  const options = vi.mocked(toast.success).mock.calls[0][1] as unknown as { action: { onClick: () => void } };
  return options.action.onClick;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(deleteSlot).mockResolvedValue({} as never);
  vi.mocked(recoverSlot).mockResolvedValue();
});

describe('deletePlaythrough', () => {
  it('deletes the current one through the core switch to the picked playthrough, then offers Undo', async () => {
    expect(await deletePlaythrough(game, slot, 2, false)).toBe(true);
    expect(deleteSlot).toHaveBeenCalledWith(1, 2);
    expect(switchSlot).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith('Main run deleted everywhere', expect.objectContaining({
      duration: 10_000,
      action: expect.objectContaining({ label: 'Undo' }) as unknown,
    }));
  });

  it('deletes one you’re not on with no replacement', async () => {
    expect(await deletePlaythrough(game, slot, undefined, true)).toBe(true);
    expect(deleteSlot).toHaveBeenCalledWith(1, undefined);
  });

  it('undoes by recovering the playthrough by uuid, without switching to it or deleting again', async () => {
    await deletePlaythrough(game, slot, 2, false);
    undo()();
    await vi.waitFor(() => { expect(recoverSlot).toHaveBeenCalledWith('main-run'); });
    expect(switchSlot).not.toHaveBeenCalled();
    expect(deleteSlot).toHaveBeenCalledTimes(1);
    expect(reportError).not.toHaveBeenCalled();
  });

  it('offers to try the Undo again when it fails, pointing at Recently deleted', async () => {
    vi.mocked(recoverSlot).mockRejectedValueOnce({ kind: 'other', message: 'offline' });
    await deletePlaythrough(game, slot, 2, false);
    undo()();
    await vi.waitFor(() => { expect(reportError).toHaveBeenCalledTimes(1); });
    const [message, , hint, action] = vi.mocked(reportError).mock.calls[0];
    expect(message).toBe('Couldn’t bring the playthrough back.');
    expect(hint).toBe('It’s still in Settings, under Recently deleted.');
    expect(action?.label).toBe('Try again');
    action?.onClick();
    await vi.waitFor(() => { expect(recoverSlot).toHaveBeenCalledTimes(2); });
    expect(switchSlot).not.toHaveBeenCalled();
  });

  it.each([
    ['the game is running', { kind: 'game_running', message: 'running' }, 2, 'Nothing was changed. Close Hollow Knight and try again.'],
    ['the saves are in use', { kind: 'saves_in_use', message: 'locked' }, 2, 'Nothing was changed. Close Hollow Knight and try again.'],
    ['a write is being put back', { kind: 'restore_incomplete', message: 'journal' }, 2, 'Your saves are safe. Worldline is putting them back by itself; try again in a moment.'],
    ['anything else goes wrong', { kind: 'other', message: 'disk' }, undefined, 'Your saves are safe. Try again in a moment.'],
  ])('reports a failure when %s, without an Undo or a repair step', async (_, failure, replacement, hint) => {
    vi.mocked(deleteSlot).mockRejectedValueOnce(failure);
    expect(await deletePlaythrough(game, slot, replacement, false)).toBe(false);
    expect(reportError).toHaveBeenCalledWith('Couldn’t delete the playthrough.', failure, hint);
    expect(toast.success).not.toHaveBeenCalled();
  });
});

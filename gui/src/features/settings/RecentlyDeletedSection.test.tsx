import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { DeletedItem } from '@/ipc/bindings/DeletedItem';
import { purgeDeleted, recoverGame, recoverSlot } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';
import { TooltipProvider } from '@/shared/ui/Tooltip';
import { daysLeft, RecentlyDeletedSection } from './RecentlyDeletedSection';
import { useRecentlyDeleted } from './queries';

vi.mock('./queries', () => ({ useRecentlyDeleted: vi.fn() }));
vi.mock('@/ipc/commands', () => ({
  exportDeletedGame: vi.fn(), pickSavePath: vi.fn(), purgeDeleted: vi.fn(), recoverGame: vi.fn(), recoverSlot: vi.fn(),
}));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }));
// Holding is a timed pointer gesture; a click stands for a completed hold.
vi.mock('@/shared/ui/HoldButton', () => ({
  HoldButton: ({ label, onComplete }: { label: string; onComplete: () => void }) => (
    <button type="button" onClick={onComplete}>{label}</button>
  ),
}));

const inDays = (n: number): string => new Date(Date.now() + n * 86_400_000).toISOString();
const game: DeletedItem = { kind: 'game', uuid: 'g1', title: 'Celeste', game_title: 'Celeste', deleted_at: inDays(-2), expires_at: inDays(27.5) };
const slot: DeletedItem = { kind: 'slot', uuid: 's1', title: 'Speedrun', game_title: 'Elden Ring', deleted_at: inDays(-29), expires_at: inDays(0.2) };

function setup(items: DeletedItem[]): void {
  vi.mocked(useRecentlyDeleted).mockReturnValue({ data: items } as unknown as ReturnType<typeof useRecentlyDeleted>);
  render(<TooltipProvider><RecentlyDeletedSection /></TooltipProvider>);
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const command of [recoverGame, recoverSlot, purgeDeleted]) vi.mocked(command).mockResolvedValue(undefined);
});

describe('daysLeft', () => {
  it('counts a part of a day as a whole one', () => {
    const now = Date.parse('2026-10-01T00:00:00Z');
    expect(daysLeft('2026-10-31T00:00:00Z', now)).toBe(30);
    expect(daysLeft('2026-10-01T01:00:00Z', now)).toBe(1);
    expect(daysLeft('2026-09-30T00:00:00Z', now)).toBe(1);
  });
});

describe('RecentlyDeletedSection', () => {
  it('is hidden while nothing is deleted', () => {
    setup([]);
    expect(screen.queryByRole('region', { name: 'Recently deleted' })).not.toBeInTheDocument();
  });

  it('lists games and playthroughs with the days they have left; only games export', () => {
    setup([game, slot]);
    const section = screen.getByRole('region', { name: 'Recently deleted' });
    expect(within(section).getByText('28 days left')).toBeInTheDocument();
    expect(within(section).getByText(/Playthrough · Elden Ring/)).toHaveTextContent('1 day left');
    expect(within(section).getByRole('button', { name: 'Export the history of Celeste' })).toBeInTheDocument();
    expect(within(section).queryByRole('button', { name: 'Export the history of Speedrun' })).not.toBeInTheDocument();
  });

  it('restores a game or a playthrough with its own command', async () => {
    setup([game, slot]);
    fireEvent.click(screen.getByRole('button', { name: 'Restore Celeste' }));
    fireEvent.click(screen.getByRole('button', { name: 'Restore Speedrun' }));
    await waitFor(() => { expect(recoverSlot).toHaveBeenCalledWith('s1'); });
    expect(recoverGame).toHaveBeenCalledWith('g1');
    expect(reportError).not.toHaveBeenCalled();
  });

  it('says why a game no longer fits and lets the row be tried again', async () => {
    const taken = { kind: 'folder_taken', message: 'taken' };
    vi.mocked(recoverGame).mockRejectedValue(taken);
    setup([game]);
    const restore = screen.getByRole('button', { name: 'Restore Celeste' });
    fireEvent.click(restore);
    await waitFor(() => {
      expect(reportError).toHaveBeenCalledWith('Couldn’t restore it.', taken, 'Another game in your library already uses this save folder.');
    });
    expect(restore).toBeEnabled();
  });

  it('deletes for good only after the hold in its dialog', async () => {
    setup([game]);
    fireEvent.click(screen.getByRole('button', { name: 'Delete Celeste now' }));
    expect(purgeDeleted).not.toHaveBeenCalled();
    const dialog = screen.getByRole('dialog', { name: 'Delete Celeste for good?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Hold to delete' }));
    await waitFor(() => { expect(purgeDeleted).toHaveBeenCalledWith('g1'); });
  });
});

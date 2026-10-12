import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactElement, RefObject } from 'react';
import { toast } from 'sonner';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DeletedItem } from '@/ipc/bindings/DeletedItem';
import { purgeDeleted, recoverGame, recoverSlot } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';
import { TooltipProvider } from '@/shared/ui/Tooltip';
import { daysLeft, RecentlyDeletedSection } from './RecentlyDeletedSection';
import { useRecentlyDeleted } from './queries';

vi.mock('./queries', () => ({ useRecentlyDeleted: vi.fn() }));
// Like the shared mock, but each tag keeps one component, so rows aren't remounted on every
// render and keep their focus as they do in the app.
vi.mock('motion/react', async () => {
  const React = await import('react');
  const tags = new Map<string, (props: { children?: React.ReactNode }) => React.ReactElement>();
  return {
    motion: new Proxy({}, {
      get: (_target, tag: string) => {
        if (!tags.has(tag)) tags.set(tag, ({ children, ...props }) => React.createElement(tag, props, children));
        return tags.get(tag);
      },
    }),
    AnimatePresence: ({ children }: { children: React.ReactNode }) => children,
    useReducedMotion: () => false,
  };
});
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
const heading: RefObject<HTMLHeadingElement | null> = { current: null };

const shows = (items: DeletedItem[]): void => {
  vi.mocked(useRecentlyDeleted).mockReturnValue({ data: items } as unknown as ReturnType<typeof useRecentlyDeleted>);
};
const view = (): ReactElement => (
  <TooltipProvider>
    <h1 ref={heading} tabIndex={-1}>Settings</h1>
    <RecentlyDeletedSection fallbackFocus={heading} />
  </TooltipProvider>
);

/** Renders `items`; the returned function shows another list, as a refetch would. */
function setup(items: DeletedItem[]): (next: DeletedItem[]) => void {
  shows(items);
  const { rerender } = render(view());
  return (next) => {
    shows(next);
    rerender(view());
  };
}

/** Clicks `button` after focusing it, as a keyboard or pointer press does in a browser. */
const press = (button: HTMLElement): void => {
  button.focus();
  fireEvent.click(button);
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(recoverGame).mockResolvedValue(true);
  for (const command of [recoverSlot, purgeDeleted]) vi.mocked(command).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe('daysLeft', () => {
  it('counts whole days only, and 0 in the last one', () => {
    const now = Date.parse('2026-10-01T00:00:00Z');
    expect(daysLeft('2026-10-31T00:00:00Z', now)).toBe(30);
    expect(daysLeft('2026-10-02T12:00:00Z', now)).toBe(1);
    expect(daysLeft('2026-10-01T23:59:00Z', now)).toBe(0);
    expect(daysLeft('2026-09-30T00:00:00Z', now)).toBe(0);
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
    expect(within(section).getByText('27 days left')).toBeInTheDocument();
    expect(within(section).getByText(/Playthrough · Elden Ring/)).toHaveTextContent('Less than a day left');
    expect(within(section).getByRole('button', { name: 'Export the history of Celeste' })).toBeInTheDocument();
    expect(within(section).queryByRole('button', { name: 'Export the history of Speedrun' })).not.toBeInTheDocument();
  });

  it('hides a row once its window ends, on the next tick', () => {
    vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
    const ending: DeletedItem = { ...slot, expires_at: new Date(Date.now() + 30_000).toISOString() };
    const ended: DeletedItem = { ...game, expires_at: new Date(Date.now() - 1).toISOString() };
    setup([ending, ended]);
    expect(screen.getByText(/Playthrough · Elden Ring/)).toHaveTextContent('Less than a day left');
    expect(screen.queryByText('Celeste')).not.toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(screen.queryByRole('region', { name: 'Recently deleted' })).not.toBeInTheDocument();
  });

  it('restores a game or a playthrough with its own command', async () => {
    setup([game, slot]);
    fireEvent.click(screen.getByRole('button', { name: 'Restore Celeste' }));
    fireEvent.click(screen.getByRole('button', { name: 'Restore Speedrun' }));
    await waitFor(() => { expect(recoverSlot).toHaveBeenCalledWith('s1'); });
    expect(recoverGame).toHaveBeenCalledWith('g1');
    expect(reportError).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith('Celeste is back');
  });

  it('says a game this PC doesn’t track is back in the cloud, not here', async () => {
    vi.mocked(recoverGame).mockResolvedValue(false);
    setup([game]);
    fireEvent.click(screen.getByRole('button', { name: 'Restore Celeste' }));
    await waitFor(() => { expect(toast.success).toHaveBeenCalledWith('Celeste is back in your cloud'); });
  });

  it('a row restored, gone, and deleted again is usable again', async () => {
    const show = setup([game]);
    fireEvent.click(screen.getByRole('button', { name: 'Restore Celeste' }));
    await waitFor(() => { expect(toast.success).toHaveBeenCalled(); });
    show([]);
    show([game]);
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Restore Celeste' })).toBeEnabled(); });
    expect(screen.getByRole('button', { name: 'Delete Celeste now' })).toBeEnabled();
  });

  it('moves the focus to the next row once one is restored, and to the heading after the last', async () => {
    const show = setup([game, slot]);
    press(screen.getByRole('button', { name: 'Restore Celeste' }));
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Restore Speedrun' })).toHaveFocus(); });
    show([slot]);
    press(screen.getByRole('button', { name: 'Restore Speedrun' }));
    await waitFor(() => { expect(heading.current).toHaveFocus(); });
  });

  it('says why a game no longer fits and lets the row be tried again', async () => {
    const taken = { kind: 'folder_taken', message: 'taken' };
    vi.mocked(recoverGame).mockRejectedValue(taken);
    setup([game]);
    const restore = screen.getByRole('button', { name: 'Restore Celeste' });
    press(restore);
    await waitFor(() => {
      expect(reportError).toHaveBeenCalledWith('Couldn’t restore it.', taken, 'Another game in your library already uses this save folder.');
    });
    expect(restore).toBeEnabled();
    expect(restore).toHaveFocus();
  });

  it('deletes for good only after the hold in its dialog, which opens on Cancel', async () => {
    setup([game]);
    fireEvent.click(screen.getByRole('button', { name: 'Delete Celeste now' }));
    expect(purgeDeleted).not.toHaveBeenCalled();
    const dialog = screen.getByRole('dialog', { name: 'Delete Celeste for good?' });
    await waitFor(() => { expect(within(dialog).getByRole('button', { name: 'Cancel' })).toHaveFocus(); });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Hold to delete' }));
    await waitFor(() => { expect(purgeDeleted).toHaveBeenCalledWith('g1'); });
    await waitFor(() => { expect(heading.current).toHaveFocus(); });
  });

  it('a failed delete now leaves the row usable, focused, and tried again', async () => {
    vi.mocked(purgeDeleted).mockRejectedValueOnce(new Error('busy'));
    setup([game]);
    const trash = screen.getByRole('button', { name: 'Delete Celeste now' });
    press(trash);
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Hold to delete' }));
    await waitFor(() => { expect(reportError).toHaveBeenCalledWith('Couldn’t delete it.', expect.any(Error)); });
    await waitFor(() => { expect(trash).toHaveFocus(); });
    expect(trash).toBeEnabled();

    press(trash);
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Hold to delete' }));
    await waitFor(() => { expect(purgeDeleted).toHaveBeenCalledTimes(2); });
    expect(reportError).toHaveBeenCalledTimes(1);
  });
});

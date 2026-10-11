import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { ContextMenu } from 'radix-ui';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame } from '@/ipc/builders';
import { deleteGame, untrackGame } from '@/ipc/commands';
import { ShellNavContext, type NavTarget, type ShellNav } from '@/features/library/shell-nav';
import { reportError } from '@/shared/errors/report';
import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { DEFAULT_PREFS } from '@/shared/prefs/schema';
import { GameContextMenu, GameMenu } from './GameMenu';

const navigate = vi.hoisted(() => vi.fn());
vi.mock('@/ipc/commands', () => ({ deleteGame: vi.fn(), untrackGame: vi.fn() }));
vi.mock('@tanstack/react-router', () => ({ useRouter: () => ({ navigate }) }));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));
// Holding is a timed pointer gesture; a click stands for a completed hold.
vi.mock('@/shared/ui/HoldButton', () => ({
  HoldButton: ({ label, onComplete }: { label: string; onComplete: () => void }) => (
    <button type="button" onClick={onComplete}>{label}</button>
  ),
}));

const game = makeGame({ id: 1, title: 'Lantern Knight', uuid: 'lantern' });

function openPopupMenu(): void {
  render(<GameMenu game={game} inCloud={false} />);
  fireEvent.keyDown(screen.getByRole('button', { name: 'More options' }), { key: 'Enter' });
  fireEvent.keyDown(screen.getByRole('menuitem', { name: 'In-game popup' }), { key: 'ArrowRight' });
}

const choice = (name: RegExp): HTMLElement => screen.getByRole('menuitemradio', { name });

describe('GameMenu in-game popup', () => {
  beforeEach(() => {
    setPref('overlay', { ...DEFAULT_PREFS.overlay, corner: 'tl', perGame: { other: 'off' } });
  });

  it('uses the default, and names the default corner', () => {
    openPopupMenu();
    expect(choice(/Use default/)).toHaveAttribute('aria-checked', 'true');
    expect(choice(/Use default/)).toHaveTextContent('Top left');
    expect(screen.getAllByRole('menuitemradio')).toHaveLength(6);
  });

  it('sets a corner or off for this game only, by uuid', () => {
    openPopupMenu();
    fireEvent.click(choice(/^Bottom left$/));
    expect(getPrefs().overlay.perGame).toEqual({ other: 'off', lantern: 'bl' });

    cleanup();
    openPopupMenu();
    fireEvent.click(choice(/Off for this game/));
    expect(getPrefs().overlay.perGame).toEqual({ other: 'off', lantern: 'off' });
  });

  it('going back to the default forgets the choice', () => {
    setPref('overlay', { ...getPrefs().overlay, perGame: { other: 'off', lantern: 'tr' } });
    openPopupMenu();
    expect(choice(/^Top right$/)).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(choice(/Use default/));
    expect(getPrefs().overlay.perGame).toEqual({ other: 'off' });
  });
});

describe('GameMenu and its right-click menu', () => {
  const names = (): string[] => screen.getAllByRole('menuitem').map((item) => item.textContent ?? '');

  function openDropdown(): void {
    render(<GameMenu game={game} inCloud />);
    fireEvent.keyDown(screen.getByRole('button', { name: 'More options' }), { key: 'Enter' });
  }

  function openContextMenu(): void {
    render(
      <GameContextMenu game={game} inCloud>
        <ContextMenu.Trigger asChild><a href="#">Lantern Knight row</a></ContextMenu.Trigger>
      </GameContextMenu>,
    );
    fireEvent.contextMenu(screen.getByText('Lantern Knight row'));
  }

  it('offer the same items', () => {
    openDropdown();
    const dropdown = names();
    cleanup();
    openContextMenu();
    expect(names()).toEqual(dropdown);
    expect(dropdown).toEqual(['Rename game', 'Change icon', 'Change color', 'Save folders0', 'In-game popup', 'Export history', 'Remove from this PC', 'Delete everywhere']);
  });

  it('open the same dialogs from the right-click menu', () => {
    openContextMenu();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename game' }));
    expect(screen.getByRole('dialog', { name: 'Rename game' })).toHaveTextContent('Rename');
    expect(screen.getByRole('textbox')).toHaveValue('Lantern Knight');

    cleanup();
    openContextMenu();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete everywhere' }));
    expect(screen.getByRole('dialog', { name: 'Delete Lantern Knight everywhere?' })).toBeInTheDocument();
  });
});

describe('Leaving a game from its menu', () => {
  const other: NavTarget = { view: 'game', gameId: 2 };
  const here: NavTarget = { view: 'game', gameId: 1 };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(untrackGame).mockResolvedValue(undefined);
    vi.mocked(deleteGame).mockResolvedValue(undefined);
  });

  // The sidebar's right-click menu, with `current` on screen.
  function openFromSidebar(current: NavTarget): void {
    render(
      <ShellNavContext value={{ current } as ShellNav}>
        <GameContextMenu game={game} inCloud>
          <ContextMenu.Trigger asChild><a href="#">Lantern Knight row</a></ContextMenu.Trigger>
        </GameContextMenu>
      </ShellNavContext>,
    );
    fireEvent.contextMenu(screen.getByText('Lantern Knight row'));
  }

  function deleteEverywhere(): void {
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete everywhere' }));
    const dialog = screen.getByRole('dialog', { name: 'Delete Lantern Knight everywhere?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Hold to delete' }));
  }

  it('goes home after removing the game on screen', async () => {
    openFromSidebar(here);
    fireEvent.click(screen.getByRole('menuitem', { name: /Remove from this PC/ }));
    await waitFor(() => { expect(navigate).toHaveBeenCalledWith({ to: '/' }); });
    expect(untrackGame).toHaveBeenCalledWith(1);
  });

  it('stays on screen after removing another game', async () => {
    openFromSidebar(other);
    fireEvent.click(screen.getByRole('menuitem', { name: /Remove from this PC/ }));
    await waitFor(() => { expect(untrackGame).toHaveBeenCalledWith(1); });
    expect(navigate).not.toHaveBeenCalled();
  });

  it('goes home after deleting the game on screen, and stays after deleting another', async () => {
    openFromSidebar(here);
    deleteEverywhere();
    await waitFor(() => { expect(navigate).toHaveBeenCalledWith({ to: '/' }); });
    expect(deleteGame).toHaveBeenCalledWith(1);

    cleanup();
    vi.clearAllMocks();
    openFromSidebar(other);
    deleteEverywhere();
    await waitFor(() => { expect(deleteGame).toHaveBeenCalledWith(1); });
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(navigate).not.toHaveBeenCalled();
  });

  it('stays, reports it, and keeps the dialog open when deleting fails', async () => {
    vi.mocked(deleteGame).mockRejectedValue(new Error('locked'));
    openFromSidebar(here);
    deleteEverywhere();
    await waitFor(() => { expect(reportError).toHaveBeenCalledWith('Couldn’t delete the game.', expect.any(Error)); });
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog', { name: 'Delete Lantern Knight everywhere?' })).toBeInTheDocument();
  });

  it('stays and reports it when removing fails', async () => {
    vi.mocked(untrackGame).mockRejectedValue(new Error('offline'));
    openFromSidebar(here);
    fireEvent.click(screen.getByRole('menuitem', { name: /Remove from this PC/ }));
    await waitFor(() => { expect(reportError).toHaveBeenCalledWith('Couldn’t remove the game.', expect.any(Error)); });
    expect(navigate).not.toHaveBeenCalled();
  });
});

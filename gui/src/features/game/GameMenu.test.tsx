import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { ContextMenu } from 'radix-ui';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame } from '@/ipc/builders';
import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { DEFAULT_PREFS } from '@/shared/prefs/schema';
import { GameContextMenu, GameMenu } from './GameMenu';

vi.mock('@/ipc/commands', () => ({}));
vi.mock('@tanstack/react-router', () => ({ useRouter: () => ({ navigate: vi.fn() }) }));
vi.mock('@/shared/ui/HoldButton', () => ({
  HoldButton: ({ label }: { label: string }) => <button type="button">{label}</button>,
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

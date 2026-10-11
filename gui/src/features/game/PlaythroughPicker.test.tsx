import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeSlot } from '@/ipc/builders';
import { PlaythroughPicker } from './PlaythroughPicker';

vi.mock('@/ipc/commands', () => ({ renameSlot: vi.fn(() => Promise.resolve()) }));

import { renameSlot } from '@/ipc/commands';

const first = makeSlot({ id: 1, name: 'Main run' });
const second = makeSlot({ id: 2, name: 'Steel Soul' });
const game = makeGame({ id: 1, title: 'Hollow Knight', slots: [first, second], active_slot: first, active_slot_id: 1 });

function openManage(): { onSwitch: ReturnType<typeof vi.fn> } {
  const onSwitch = vi.fn();
  render(<PlaythroughPicker game={game} disabled={false} onSwitch={onSwitch} onCreate={vi.fn()} />);
  fireEvent.keyDown(screen.getByRole('button', { name: 'Playthrough' }), { key: 'Enter' });
  fireEvent.click(screen.getByRole('menuitem', { name: 'Manage playthroughs…' }));
  return { onSwitch };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Manage playthroughs', () => {
  it('lists every playthrough and marks the current one', () => {
    openManage();
    const dialog = screen.getByRole('dialog', { name: 'Playthroughs' });
    const rows = within(dialog).getAllByRole('listitem');
    expect(rows.map((r) => r.textContent)).toEqual(['Main runCurrent · Empty', 'Steel SoulEmpty']);
  });

  it('renames a playthrough without switching to it', async () => {
    const { onSwitch } = openManage();
    fireEvent.click(screen.getByRole('button', { name: 'Rename Steel Soul' }));
    const input = screen.getByRole('textbox', { name: 'Playthrough name' });
    expect(input).toHaveValue('Steel Soul');
    fireEvent.change(input, { target: { value: '  Radiant  ' } });
    fireEvent.submit(input);
    await waitFor(() => { expect(renameSlot).toHaveBeenCalledWith(2, 'Radiant'); });
    expect(onSwitch).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog', { name: 'Playthroughs' })).toBeInTheDocument();
  });

  it('leaves the name alone on Escape, an empty field or the same name', () => {
    openManage();
    fireEvent.click(screen.getByRole('button', { name: 'Rename Main run' }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Other' } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'Playthroughs' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Rename Main run' }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '   ' } });
    fireEvent.submit(screen.getByRole('textbox'));
    fireEvent.click(screen.getByRole('button', { name: 'Rename Main run' }));
    fireEvent.blur(screen.getByRole('textbox'));
    expect(renameSlot).not.toHaveBeenCalled();
  });
});

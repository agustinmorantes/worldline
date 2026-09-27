import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SlotSelector } from './SlotSelector';
import { Slot } from '@/models/slot';

function slot(id: number, name: string): Slot {
  const s = new Slot(name);
  s.id = id;
  return s;
}

type SlotSelectorProps = Parameters<typeof SlotSelector>[0];

function renderSelector(overrides: Partial<SlotSelectorProps> = {}): SlotSelectorProps {
  const props: SlotSelectorProps = {
    slots: [slot(1, 'Default Slot'), slot(2, 'Challenge Run')],
    activeSlotId: 1,
    onSwitch: vi.fn().mockResolvedValue(undefined),
    onCreate: vi.fn().mockResolvedValue(undefined),
    onRename: vi.fn().mockResolvedValue(undefined),
    onDelete: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(<SlotSelector {...props} />);
  return props;
}

describe('SlotSelector', () => {
  it('warns and waits for confirmation before switching, because switching writes save files', async () => {
    const { onSwitch, slots } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: 'Challenge Run' }));

    // The click alone must not touch the disk.
    expect(onSwitch).not.toHaveBeenCalled();

    // The user has to be told what is about to happen, and to close the game
    // first — the same warning the restore flow gives.
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('Challenge Run');
    expect(dialog).toHaveTextContent(/close the game/i);
    expect(dialog).toHaveTextContent(/overwritten/i);

    fireEvent.click(screen.getByRole('button', { name: /switch slot/i }));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onSwitch).toHaveBeenCalledWith(slots[1]);
  });

  it('does not switch when the confirmation is dismissed', async () => {
    const { onSwitch } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: 'Challenge Run' }));
    await screen.findByRole('dialog');
    fireEvent.click(screen.getByRole('button', { name: /^cancel$/i }));

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it('does not prompt when the already-active slot is clicked', () => {
    const { onSwitch } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: 'Default Slot' }));

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it('disables slot actions while an operation runs', () => {
    renderSelector({ slots: [slot(1, 'Default Slot')], disabled: true });

    expect(screen.getByRole('button', { name: 'Default Slot' })).toBeDisabled();
    expect(screen.getByRole('button', { name: /rename default slot/i })).toBeDisabled();
    expect(screen.getByRole('button', { name: /^new slot$/i })).toBeDisabled();
  });

  it('renames a slot, seeded with its current name', async () => {
    const { onRename, slots } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: /rename challenge run/i }));

    const input = await screen.findByRole('textbox');
    expect(input).toHaveValue('Challenge Run');

    fireEvent.change(input, { target: { value: 'Nightmare Run' } });
    fireEvent.click(screen.getByRole('button', { name: /^rename$/i }));

    expect(onRename).toHaveBeenCalledTimes(1);
    expect(onRename).toHaveBeenCalledWith(slots[1], 'Nightmare Run');
  });

  it('names the slot it will switch to when the active slot is deleted', async () => {
    const { onDelete, slots } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: /delete default slot/i }));

    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('Default Slot');
    // Deleting the slot you are on has to move you somewhere, and the user is
    // told where before it happens.
    expect(dialog).toHaveTextContent('Challenge Run');

    fireEvent.click(screen.getByRole('button', { name: /^delete slot$/i }));

    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledWith(slots[0], 2);
  });

  it('deletes an inactive slot without moving the active one', async () => {
    const { onDelete, slots } = renderSelector();

    fireEvent.click(screen.getByRole('button', { name: /delete challenge run/i }));
    await screen.findByRole('dialog');
    fireEvent.click(screen.getByRole('button', { name: /^delete slot$/i }));

    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledWith(slots[1], undefined);
  });

  it('refuses to delete the last remaining slot', () => {
    renderSelector({ slots: [slot(1, 'Default Slot')] });

    expect(screen.getByRole('button', { name: /delete default slot/i })).toBeDisabled();
  });
});

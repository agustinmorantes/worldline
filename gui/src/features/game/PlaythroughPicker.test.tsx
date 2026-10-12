import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { makeGame, makeSlot } from '@/ipc/builders';
import { TooltipProvider } from '@/shared/ui/Tooltip';
import { PlaythroughPicker } from './PlaythroughPicker';

vi.mock('@/ipc/commands', () => ({
  renameSlot: vi.fn(() => Promise.resolve()),
  getDevice: vi.fn(() => Promise.reject(new Error('not in this test'))),
  getSyncSettings: vi.fn(() => Promise.reject(new Error('not in this test'))),
}));

import { renameSlot } from '@/ipc/commands';

const first = makeSlot({ id: 1, name: 'Main run' });
const second = makeSlot({ id: 2, name: 'Steel Soul' });
const game = makeGame({ id: 1, title: 'Hollow Knight', slots: [first, second], active_slot: first, active_slot_id: 1 });

type OnDelete = (slot: unknown, replacement: number | undefined) => Promise<boolean>;
interface Options { of?: FullGame; running?: boolean; onDelete?: ReturnType<typeof vi.fn<OnDelete>> }
interface Handlers { onSwitch: ReturnType<typeof vi.fn>; onDelete: ReturnType<typeof vi.fn<OnDelete>> }

function renderPicker({ of = game, running = false, onDelete = vi.fn<OnDelete>(() => Promise.resolve(true)) }: Options = {}): Handlers {
  const onSwitch = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <TooltipProvider>
        <PlaythroughPicker game={of} disabled={false} running={running} onSwitch={onSwitch} onCreate={vi.fn()} onDelete={onDelete} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  fireEvent.keyDown(screen.getByRole('button', { name: 'Playthrough' }), { key: 'Enter' });
  return { onSwitch, onDelete };
}

function openManage(options: Options = {}): Handlers {
  const handlers = renderPicker(options);
  fireEvent.click(screen.getByRole('menuitem', { name: 'Manage playthroughs…' }));
  return handlers;
}

const deleteButton = (name: string): HTMLElement => screen.getByRole('button', { name: `Delete ${name}` });
const question = (name: string): HTMLElement => screen.getByRole('group', { name: `Delete ${name} everywhere?` });

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

  it('gives focus back to the row’s Rename button after Enter or Escape', async () => {
    openManage();
    const rename = screen.getByRole('button', { name: 'Rename Steel Soul' });
    fireEvent.click(rename);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Rename Steel Soul' })).toHaveFocus(); });

    fireEvent.click(screen.getByRole('button', { name: 'Rename Steel Soul' }));
    fireEvent.submit(screen.getByRole('textbox'));
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Rename Steel Soul' })).toHaveFocus(); });
  });

  it('gives focus back to the playthrough chip when it closes', async () => {
    openManage();
    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(screen.getByRole('button', { name: 'Playthrough' })).toHaveFocus();
  });

  it('scrolls a long list between a fixed heading and footer', () => {
    openManage();
    const dialog = screen.getByRole('dialog', { name: 'Playthroughs' });
    expect(dialog).toHaveClass('max-h-[calc(100dvh-2rem)]');
    expect(within(dialog).getByRole('list')).toHaveClass('min-h-0', 'overflow-y-auto');
  });
});

describe('Deleting a playthrough', () => {
  it('deletes one you’re not on without a switch, after a confirm that says everywhere and 30 days', async () => {
    const { onDelete, onSwitch } = openManage();
    fireEvent.click(deleteButton('Steel Soul'));
    const asked = question('Steel Soul');
    expect(asked).toHaveTextContent('It leaves every device. You can restore it from Recently deleted for 30 days.');
    expect(within(asked).queryByRole('radio')).not.toBeInTheDocument();
    expect(within(asked).getByRole('button', { name: 'Cancel' })).toHaveFocus();

    fireEvent.click(within(asked).getByRole('button', { name: 'Delete' }));
    await waitFor(() => { expect(onDelete).toHaveBeenCalledWith(second, undefined); });
    await waitFor(() => { expect(screen.queryByRole('group')).not.toBeInTheDocument(); });
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it('asks which playthrough to switch to before deleting the current one, and assumes none', async () => {
    const third = makeSlot({ id: 3, name: 'Speedrun' });
    const { onDelete } = openManage({ of: { ...game, slots: [first, second, third] } });
    fireEvent.click(deleteButton('Main run'));
    const asked = question('Main run');
    expect(asked).toHaveTextContent('everywhere');
    const radios = within(asked).getAllByRole('radio');
    expect(radios.map((r) => r.closest('label')?.textContent)).toEqual(['Steel Soul', 'Speedrun']);
    expect(radios.every((r) => !(r as HTMLInputElement).checked)).toBe(true);
    expect(radios[0]).toHaveFocus();

    const confirm = within(asked).getByRole('button', { name: 'Delete' });
    expect(confirm).toHaveAttribute('aria-disabled', 'true');
    fireEvent.click(confirm);
    expect(onDelete).not.toHaveBeenCalled();

    fireEvent.click(within(asked).getByRole('radio', { name: 'Speedrun' }));
    const ready = within(question('Main run')).getByRole('button', { name: 'Delete' });
    expect(ready).toHaveAttribute('aria-disabled', 'false');
    fireEvent.click(ready);
    await waitFor(() => { expect(onDelete).toHaveBeenCalledWith(first, 3); });
  });

  it('keeps the last playthrough, saying why', async () => {
    const { onDelete } = openManage({ of: { ...game, slots: [first] } });
    const only = deleteButton('Main run');
    expect(only).toHaveAttribute('aria-disabled', 'true');
    fireEvent.pointerMove(only);
    expect(await screen.findByRole('tooltip')).toHaveTextContent('A game keeps at least one playthrough.');
    fireEvent.click(only);
    expect(screen.queryByRole('group')).not.toBeInTheDocument();
    expect(onDelete).not.toHaveBeenCalled();
  });

  it('cancels with Cancel or Esc, giving focus back to the row’s Delete and keeping the dialog', async () => {
    const { onDelete } = openManage();
    fireEvent.click(deleteButton('Steel Soul'));
    fireEvent.click(within(question('Steel Soul')).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('group')).not.toBeInTheDocument();
    await waitFor(() => { expect(deleteButton('Steel Soul')).toHaveFocus(); });

    fireEvent.click(deleteButton('Main run'));
    fireEvent.keyDown(within(question('Main run')).getAllByRole('radio')[0], { key: 'Escape' });
    expect(screen.queryByRole('group')).not.toBeInTheDocument();
    expect(screen.getByRole('dialog', { name: 'Playthroughs' })).toBeInTheDocument();
    await waitFor(() => { expect(deleteButton('Main run')).toHaveFocus(); });
    expect(onDelete).not.toHaveBeenCalled();
  });

  it('keeps the question open, ready to try again, when the delete fails', async () => {
    const { onDelete } = openManage({ onDelete: vi.fn<OnDelete>(() => Promise.resolve(false)) });
    fireEvent.click(deleteButton('Main run'));
    fireEvent.click(within(question('Main run')).getByRole('radio', { name: 'Steel Soul' }));
    fireEvent.click(within(question('Main run')).getByRole('button', { name: 'Delete' }));
    await waitFor(() => { expect(onDelete).toHaveBeenCalledWith(first, 2); });
    const confirm = within(question('Main run')).getByRole('button', { name: 'Delete' });
    await waitFor(() => { expect(confirm).toHaveFocus(); });
    expect(confirm).toHaveAttribute('aria-disabled', 'false');
  });

  it('while the game runs, refuses deleting the current one but deletes and renames the others', async () => {
    const { onDelete } = openManage({ running: true });
    const current = deleteButton('Main run');
    expect(current).toHaveAttribute('aria-disabled', 'true');
    fireEvent.pointerMove(current);
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Close Hollow Knight first: deleting the playthrough you’re on switches your saves.');
    fireEvent.click(current);
    expect(screen.queryByRole('group')).not.toBeInTheDocument();

    fireEvent.click(deleteButton('Steel Soul'));
    fireEvent.click(within(question('Steel Soul')).getByRole('button', { name: 'Delete' }));
    await waitFor(() => { expect(onDelete).toHaveBeenCalledWith(second, undefined); });

    fireEvent.click(screen.getByRole('button', { name: 'Rename Steel Soul' }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Radiant' } });
    fireEvent.submit(screen.getByRole('textbox'));
    await waitFor(() => { expect(renameSlot).toHaveBeenCalledWith(2, 'Radiant'); });
  });
});

describe('Switching while the game runs', () => {
  it('is off, saying why, and managing stays open', () => {
    const { onSwitch } = renderPicker({ running: true });
    expect(screen.getByText('Close Hollow Knight to switch playthroughs.')).toBeInTheDocument();
    expect(screen.getByRole('menuitemradio', { name: /Steel Soul/ })).toHaveAttribute('aria-disabled', 'true');
    expect(screen.getByRole('menuitem', { name: 'New playthrough' })).toHaveAttribute('aria-disabled', 'true');
    expect(screen.getByRole('menuitem', { name: 'Manage playthroughs…' })).not.toHaveAttribute('aria-disabled');
    fireEvent.click(screen.getByRole('menuitemradio', { name: /Steel Soul/ }));
    expect(onSwitch).not.toHaveBeenCalled();
  });
});

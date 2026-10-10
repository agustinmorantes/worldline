import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Dialog } from 'radix-ui';
import type { NamingCard } from '@/ipc/events';
import { NamingHost } from './NamingHost';

const handlers = new Map<string, (payload: unknown) => void>();
const navigate = vi.fn();
const animate = vi.fn();
vi.mock('@tanstack/react-router', () => ({ useRouter: () => ({ navigate }) }));
vi.mock('@/ipc/events', () => ({
  on: vi.fn((event: string, handler: (payload: unknown) => void) => {
    handlers.set(event, handler);
    return Promise.resolve(() => { handlers.delete(event); });
  }),
}));
vi.mock('@/ipc/commands', () => ({ nameItClosed: vi.fn() }));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));

import { nameItClosed } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';

const card: NamingCard = {
  session: 8, gameId: 7, versionId: 40, version: 43, auto: 'Kept in game, 21:04', title: 'Lantern Knight',
  accentHue: 28, corner: 'br', motion: 'full', language: 'en',
};
const field = (): HTMLInputElement => screen.getByRole('textbox', { name: 'Checkpoint name' });
const advance = (ms: number): Promise<void> => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

/** The session row's tag chip, as `SessionList` marks it. */
function row(): HTMLElement {
  const tag = document.createElement('span');
  tag.dataset.versions = '39 40';
  document.body.append(tag);
  return tag;
}

async function open(): Promise<void> {
  render(<NamingHost />);
  await waitFor(() => { expect(handlers.has('name-it-open')).toBe(true); });
  vi.useFakeTimers();
  act(() => { handlers.get('name-it-open')?.(card); });
}

beforeEach(() => {
  handlers.clear();
  navigate.mockClear();
  vi.mocked(nameItClosed).mockReset();
  vi.mocked(reportError).mockClear();
  animate.mockClear();
  Element.prototype.animate = animate;
});
afterEach(() => {
  vi.useRealTimers();
  document.querySelectorAll('[data-versions]').forEach((el) => { el.remove(); });
});

describe('NamingHost', () => {
  it('opens the game’s view with the card over it', async () => {
    await open();
    expect(navigate).toHaveBeenCalledWith({ to: '/game/$gameId', params: { gameId: '7' }, search: {} });
    expect(document.querySelector('.nb')).toHaveClass('app', 'open');
    expect(field()).toHaveValue('Kept in game, 21:04');
  });

  it('when the focus went back to the game, only folds and pops the row’s tag', async () => {
    const tag = row();
    vi.mocked(nameItClosed).mockResolvedValue({ handed: true, error: null });
    await open();
    fireEvent.change(field(), { target: { value: 'Boss door' } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(nameItClosed).toHaveBeenCalledWith(8, 'save', 'Boss door');
    await advance(420);
    expect(document.querySelector('.nb')).toHaveClass('folded');
    await advance(120);
    expect(document.querySelector('.nb')).toBeNull();
    expect(animate).toHaveBeenCalledOnce();
    expect(animate.mock.contexts[0]).toBe(tag);
  });

  it('when the game is gone, flies onto the row', async () => {
    row();
    vi.mocked(nameItClosed).mockResolvedValue({ handed: false, error: null });
    await open();
    fireEvent.keyDown(field(), { key: 'Escape' });
    expect(nameItClosed).toHaveBeenCalledWith(8, 'escape', '');
    await advance(420);
    expect(document.querySelector('.nb')).toHaveClass('flying');
    await advance(460);
    expect(document.querySelector('.nb')).toBeNull();
  });

  it('a focus loss saves without asking for anything else', async () => {
    vi.mocked(nameItClosed).mockResolvedValue({ handed: false, error: null });
    await open();
    fireEvent.blur(field());
    expect(nameItClosed).toHaveBeenCalledWith(8, 'blur', 'Kept in game, 21:04');
    // No row on screen: nowhere to fly.
    await advance(420);
    expect(document.querySelector('.nb')).toHaveClass('folded');
    expect(reportError).not.toHaveBeenCalled();
  });

  it('says when the name couldn’t be saved, and what the checkpoint keeps', async () => {
    vi.mocked(nameItClosed).mockResolvedValue({ handed: true, error: 'database is locked' });
    await open();
    fireEvent.change(field(), { target: { value: 'Boss door' } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    await advance(0);
    expect(reportError).toHaveBeenCalledWith('Couldn’t name the checkpoint.', 'database is locked', 'It keeps “Kept in game, 21:04”.');
  });

  it('takes the focus from a modal already open, and keeps Esc its own', async () => {
    const other = (
      <Dialog.Root open>
        <Dialog.Portal>
          <Dialog.Content aria-describedby={undefined}>
            <Dialog.Title>Tour</Dialog.Title>
            <button type="button">Next</button>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    );
    vi.mocked(nameItClosed).mockResolvedValue({ handed: false, error: null });
    render(other);
    await open();
    await advance(30);
    expect(field()).toHaveFocus();
    expect(screen.getByRole('dialog', { name: 'Name this checkpoint' })).toBeInTheDocument();
    fireEvent.keyDown(field(), { key: 'Escape' });
    expect(nameItClosed).toHaveBeenCalledWith(8, 'escape', '');
    // The modal under it is still open.
    expect(screen.getByRole('button', { name: 'Next', hidden: true })).toBeInTheDocument();
  });
});

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Events, NamingCard, OverlayShow } from '@/ipc/events';
import { cornerFlight, Overlay } from './Overlay';

const handlers = new Map<string, (payload: unknown) => void>();
vi.mock('@/ipc/events', () => ({
  on: vi.fn((event: string, handler: (payload: unknown) => void) => {
    handlers.set(event, handler);
    return Promise.resolve(() => { handlers.delete(event); });
  }),
  signal: vi.fn().mockResolvedValue(undefined),
}));

import { signal } from '@/ipc/events';

function emit<K extends keyof Events>(event: K, payload: Events[K]): void {
  act(() => { handlers.get(event)?.(payload); });
}

const show = (over: Partial<OverlayShow> = {}): OverlayShow => ({
  id: 1, swap: false, kind: 'checkpoint', title: 'Lantern Knight', accentHue: 28, version: 42,
  corner: 'br', quiet: false, motion: 'full', language: 'en', durationMs: 3200, ...over,
}) as OverlayShow;

async function mount(): Promise<void> {
  render(<Overlay />);
  await waitFor(() => { expect(signal).toHaveBeenCalledWith('overlay-ready', null); });
}

const popup = (): HTMLElement => screen.getByRole('status');

/** jsdom has no AnimationEvent, so React listens for the prefixed name there. */
function animationEnd(el: Element, animationName: string): void {
  fireEvent(el, Object.assign(new Event('webkitAnimationEnd', { bubbles: true }), { animationName }));
}

describe('Overlay', () => {
  beforeEach(() => {
    handlers.clear();
    vi.mocked(signal).mockClear();
  });

  it('says it is ready only once it listens', async () => {
    await mount();
    expect(handlers.has('overlay-show')).toBe(true);
    expect(handlers.has('overlay-leave')).toBe(true);
  });

  it('shows the checkpoint in its corner, rolling from the number before', async () => {
    await mount();
    emit('overlay-show', show({ corner: 'tl' }));
    expect(popup()).toHaveClass('ov', 'c-tl', 'enter');
    expect(popup()).toHaveTextContent('Checkpoint#41#42Lantern Knight');
    expect(popup().style.getPropertyValue('--dur')).toBe('3200ms');
  });

  it('shows a new timeline in the branch colour, with what it branched off', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'newTimeline', version: 43, from: { version_number: 40, tag: 'Before the bridge' } }));
    expect(popup()).toHaveClass('k-timeline');
    expect(popup()).toHaveTextContent('New timeline#42#43Branched off “Before the bridge”');
    expect(popup().style.getPropertyValue('--lane')).toBe('oklch(80% 0.13 183)');
    expect(popup().querySelector('.burst')).not.toBeNull();
    emit('overlay-show', show({ id: 2, kind: 'newTimeline', version: 43, from: { version_number: 40, tag: null } }));
    expect(popup()).toHaveTextContent('Branched off #40');
  });

  it('shows the session start with the last checkpoint, or none yet', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'sessionStart', lastAt: Date.now() - 2 * 86_400_000 }));
    expect(popup()).toHaveClass('k-recording');
    expect(popup()).toHaveTextContent('Worldline is recordingLast checkpoint 2 days ago');
    expect(popup().querySelector('.num')).toBeNull();
    emit('overlay-show', show({ id: 2, kind: 'sessionStart', lastAt: null }));
    expect(popup()).toHaveTextContent('No checkpoints yet');
  });

  it('warns that another device is ahead, with a badge', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'deviceAhead', device: 'Steam Deck', count: 3, durationMs: 8000 }));
    expect(popup()).toHaveClass('k-ahead');
    expect(popup()).toHaveTextContent('Steam Deck is ahead3 newer checkpoints · quit and sync first');
    expect(popup().querySelector('.badge path')).toHaveAttribute('d', 'M7 3.2v4.6M7 10.6v.2');
    emit('overlay-show', show({ id: 2, kind: 'deviceAhead', device: 'Steam Deck', count: 1, language: 'es' }));
    expect(popup()).toHaveTextContent('Steam Deck va por delante1 punto de control más nuevo · sal y sincroniza primero');
  });

  it('says a failed checkpoint left the game save untouched, with why', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'captureFailed', reason: 'disk_full', version: null }));
    expect(popup()).toHaveClass('k-failed');
    expect(popup()).toHaveTextContent('Checkpoint not savedDisk full · your game save is untouched');
    expect(popup().querySelector('.badge path')).toHaveAttribute('d', 'M4.5 4.5l5 5M9.5 4.5l-5 5');
    emit('overlay-show', show({ id: 2, kind: 'captureFailed', reason: 'other', version: null, language: 'es' }));
    expect(popup()).toHaveTextContent('No se guardó el punto de controlNo se pudo escribir · tu partida está intacta');
  });

  it('says a kept checkpoint that was saved but couldn’t be named wasn’t kept, and where it is', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'captureFailed', reason: 'disk_full', version: 13 }));
    expect(popup()).toHaveClass('k-failed');
    expect(popup()).toHaveTextContent('Checkpoint not keptDisk full · saved as #13');
    emit('overlay-show', show({ id: 2, kind: 'captureFailed', reason: 'other', version: 13, language: 'es' }));
    expect(popup()).toHaveTextContent('No se conservó el punto de controlNo se pudo escribir · guardado como #13');
  });

  it('shows a kept checkpoint as a diamond, its number still', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'kept', version: 42, name: null }));
    expect(popup()).toHaveClass('k-pinned');
    expect(popup()).toHaveTextContent('Kept forever#42Rename it later in Worldline');
    expect(popup().querySelector('.roll')).toBeNull();
    emit('overlay-show', show({ id: 2, kind: 'kept', version: 42, name: 'Boss door' }));
    expect(popup()).toHaveTextContent('“Boss door”');
  });

  it('only checkpoints and warnings get their badge or number', async () => {
    await mount();
    emit('overlay-show', show());
    expect(popup()).toHaveClass('k-checkpoint');
    expect(popup().querySelector('.badge')).toBeNull();
  });

  it('a swap from a popup without a number rolls from the number before', async () => {
    await mount();
    emit('overlay-show', show({ kind: 'sessionStart', lastAt: null }));
    emit('overlay-show', show({ id: 2, swap: true, version: 43 }));
    expect(popup()).toHaveClass('swap', 'k-checkpoint');
    expect(popup()).toHaveTextContent('Checkpoint#42#43');
  });

  it('speaks the language it is sent', async () => {
    await mount();
    emit('overlay-show', show({ language: 'es' }));
    expect(popup()).toHaveTextContent('Punto de control');
  });

  it('swaps in place: the same popup rolls on from the number it showed', async () => {
    await mount();
    emit('overlay-show', show());
    const first = popup();
    const parts = (): (Element | null)[] =>
      ['.tile', '.ring', '.lines', '.roll', '.life'].map((part) => popup().querySelector(part));
    const before = parts();
    const txt = popup().querySelector('.txt');
    emit('overlay-show', show({ id: 2, swap: true, version: 44 }));
    expect(popup()).toBe(first);
    expect(popup()).toHaveClass('swap');
    expect(popup()).toHaveTextContent('#42#44');
    // What a swap animates is new, so its animations play again; the text block keeps its
    // width to transition from.
    parts().forEach((part, i) => { expect(part).not.toBe(before[i]); });
    expect(popup().querySelector('.txt')).toBe(txt);
    const again = parts();
    emit('overlay-show', show({ id: 3, swap: true, version: 45 }));
    parts().forEach((part, i) => { expect(part).not.toBe(again[i]); });
  });

  it('quiet mode shows only the mark', async () => {
    await mount();
    emit('overlay-show', show({ quiet: true }));
    expect(popup()).toHaveClass('quiet');
    expect(popup()).not.toHaveTextContent('Checkpoint');
  });

  it('reduced motion only fades', async () => {
    await mount();
    emit('overlay-show', show({ motion: 'reduced' }));
    expect(popup()).toHaveClass('rm');
  });

  it('says done when its exit animation ends, not before', async () => {
    await mount();
    emit('overlay-show', show());
    // The entry's own end means nothing.
    animationEnd(popup(), 'rise');
    emit('overlay-leave', { id: 1, fast: false });
    expect(popup()).toHaveClass('out');
    // A child's animation ending isn't the exit.
    const txt = popup().querySelector('.txt');
    if (txt) animationEnd(txt, 'shut');
    expect(signal).not.toHaveBeenCalledWith('overlay-done', 1);

    // Nor is the entry ending late, after the leave came.
    animationEnd(popup(), 'rise');
    expect(signal).not.toHaveBeenCalledWith('overlay-done', 1);

    animationEnd(popup(), 'sink');
    expect(signal).toHaveBeenCalledWith('overlay-done', 1);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('fades at once when the game leaves the front', async () => {
    await mount();
    emit('overlay-show', show());
    emit('overlay-leave', { id: 1, fast: true });
    expect(popup()).toHaveClass('gone');
  });

  it('switches the fold to the quick fade when the game leaves the front during it', async () => {
    await mount();
    emit('overlay-show', show());
    emit('overlay-leave', { id: 1, fast: false });
    emit('overlay-leave', { id: 1, fast: true });
    expect(popup()).toHaveClass('gone');
    expect(signal).not.toHaveBeenCalledWith('overlay-done', 1);
    animationEnd(popup(), 'fadeOut');
    expect(signal).toHaveBeenCalledWith('overlay-done', 1);
  });

  it('answers a leave for a popup it no longer has at once', async () => {
    await mount();
    emit('overlay-show', show({ id: 3 }));
    emit('overlay-leave', { id: 2, fast: false });
    expect(signal).toHaveBeenCalledWith('overlay-done', 2);
    expect(popup()).toHaveClass('enter');
  });

  it('a show during the exit starts a new popup, and the old exit never reports', async () => {
    await mount();
    emit('overlay-show', show());
    emit('overlay-leave', { id: 1, fast: false });
    const leaving = popup();
    emit('overlay-show', show({ id: 2, version: 43 }));
    expect(popup()).not.toBe(leaving);
    expect(popup()).toHaveClass('enter');
    expect(popup()).not.toHaveClass('swap');
    animationEnd(popup(), 'rise');
    expect(signal).not.toHaveBeenCalledWith('overlay-done', 1);
  });
});

const card = (over: Partial<NamingCard> = {}): NamingCard => ({
  session: 5, gameId: 7, versionId: 40, version: 43, auto: 'Kept in game, 21:04', title: 'Lantern Knight',
  accentHue: 28, corner: 'br', motion: 'full', language: 'en', ...over,
});
const field = (): HTMLInputElement => screen.getByRole('textbox', { name: 'Checkpoint name' });
const advance = (ms: number): Promise<void> => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

describe('Overlay naming card', () => {
  beforeEach(() => {
    handlers.clear();
    vi.mocked(signal).mockClear();
  });
  afterEach(() => { vi.useRealTimers(); });

  it('listens for the card and the game taking the focus back', async () => {
    await mount();
    expect(handlers.has('naming-open')).toBe(true);
    expect(handlers.has('naming-leave')).toBe(true);
  });

  it('takes the popup’s place without reporting its exit', async () => {
    await mount();
    emit('overlay-show', show());
    emit('naming-open', card());
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Name this checkpoint' })).toBeInTheDocument();
    expect(signal).not.toHaveBeenCalledWith('overlay-done', 1);
  });

  it('says how it closed, then that its tile landed in the corner, and stays until the popup', async () => {
    await mount();
    vi.useFakeTimers();
    emit('naming-open', card());
    fireEvent.change(field(), { target: { value: 'Boss door' } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(signal).toHaveBeenCalledWith('naming-closed', { session: 5, how: 'save', name: 'Boss door' });
    await advance(420);
    const host = screen.getByRole('dialog').parentElement;
    if (host === null) throw new Error('No card');
    const { x, y } = cornerFlight('br', window.innerWidth, window.innerHeight);
    expect(host.style.getPropertyValue('--fx')).toBe(`${x.toString()}px`);
    expect(host.style.getPropertyValue('--fy')).toBe(`${y.toString()}px`);
    expect(signal).not.toHaveBeenCalledWith('naming-landed', 5);
    await advance(440);
    expect(signal).toHaveBeenCalledWith('naming-landed', 5);
    expect(document.querySelector('.nb')).not.toBeNull();

    emit('overlay-show', show({ id: 9, kind: 'kept', version: 43, name: 'Boss door', landed: true }));
    expect(document.querySelector('.nb')).toBeNull();
    expect(popup()).toHaveClass('landed', 'enter');
    expect(popup()).toHaveTextContent('“Boss door”');
  });

  it('saves as a focus loss when Rust says the game took the focus back', async () => {
    await mount();
    emit('naming-open', card());
    emit('naming-leave', 4);
    expect(signal).not.toHaveBeenCalledWith('naming-closed', expect.anything());
    emit('naming-leave', 5);
    expect(signal).toHaveBeenCalledWith('naming-closed', { session: 5, how: 'blur', name: 'Kept in game, 21:04' });
  });

  it('flies its tile onto the popup’s tile in each corner', () => {
    // The card's tile sits 80px above the centre of a 1280x720 client area: at (640, 280).
    expect(cornerFlight('br', 1280, 720)).toEqual({ x: 1240 - 640, y: 680 - 280 });
    expect(cornerFlight('bl', 1280, 720)).toEqual({ x: 40 - 640, y: 680 - 280 });
    expect(cornerFlight('tr', 1280, 720)).toEqual({ x: 1240 - 640, y: 40 - 280 });
    expect(cornerFlight('tl', 1280, 720)).toEqual({ x: 40 - 640, y: 40 - 280 });
  });
});

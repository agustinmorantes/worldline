import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Events, OverlayShow } from '@/ipc/events';
import { Overlay } from './Overlay';

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
});

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

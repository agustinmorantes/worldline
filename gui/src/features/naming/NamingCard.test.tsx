import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { NamingCard as Card } from '@/ipc/events';
import { NamingCard, type NamingCardProps } from './NamingCard';

const card = (over: Partial<Card> = {}): Card => ({
  session: 3, gameId: 7, versionId: 40, version: 43, auto: 'Kept in game, 21:04', title: 'Lantern Knight',
  accentHue: 28, corner: 'br', motion: 'full', language: 'en', ...over,
});

function setup(props: Partial<NamingCardProps> = {}): { onClose: ReturnType<typeof vi.fn>; onDone: ReturnType<typeof vi.fn>; rerender: (left: boolean) => void } {
  const onClose = vi.fn(() => ({ x: 600, y: 300 }));
  const onDone = vi.fn();
  const all = { card: card(), place: 'overlay' as const, onClose, onDone, ...props };
  const { rerender } = render(<NamingCard {...all} />);
  return { onClose: (props.onClose ?? onClose) as ReturnType<typeof vi.fn>, onDone, rerender: (left) => { rerender(<NamingCard {...all} left={left} />); } };
}

const field = (): HTMLInputElement => screen.getByRole('textbox', { name: 'Checkpoint name' });
function host(): HTMLElement {
  const el = document.querySelector<HTMLElement>('.nb');
  if (el === null) throw new Error('No card');
  return el;
}
const advance = (ms: number): Promise<void> => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe('NamingCard', () => {
  it('opens on the checkpoint with the automatic name focused and selected', () => {
    setup();
    expect(screen.getByRole('dialog', { name: 'Name this checkpoint' })).toHaveTextContent('#43 · Lantern Knight · just now');
    expect(field()).toHaveFocus();
    expect(field().value).toBe('Kept in game, 21:04');
    expect([field().selectionStart, field().selectionEnd]).toEqual([0, 'Kept in game, 21:04'.length]);
    expect(field()).toHaveAttribute('maxLength', '60');
    expect(screen.getByText(/keeps “Kept in game, 21:04”/)).toBeInTheDocument();
    expect(host()).toHaveClass('nb', 'overlay', 'open');
  });

  it('speaks Spanish when the app does', () => {
    setup({ card: card({ language: 'es' }) });
    expect(screen.getByRole('dialog', { name: 'Ponle nombre a este punto de control' })).toHaveTextContent('#43 · Lantern Knight · ahora mismo');
    expect(screen.getByRole('button', { name: 'Guardar nombre' })).toBeInTheDocument();
    expect(screen.getByText(/conserva «Kept in game, 21:04»/)).toBeInTheDocument();
  });

  it('Enter saves what was typed, folds, flies, then is done', async () => {
    const { onClose, onDone } = setup();
    fireEvent.change(field(), { target: { value: '  Before the boss ' } });
    fireEvent.keyDown(field(), { key: 'Enter' });
    expect(onClose).toHaveBeenCalledWith('save', '  Before the boss ');
    expect(host()).toHaveClass('closing');
    await advance(420);
    expect(host()).toHaveClass('flying');
    expect(host().style.getPropertyValue('--fx')).toBe('600px');
    expect(host().style.getPropertyValue('--fy')).toBe('300px');
    expect(onDone).not.toHaveBeenCalled();
    await advance(440);
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('Esc keeps the automatic name, discarding the typing', () => {
    const { onClose } = setup();
    fireEvent.change(field(), { target: { value: 'Typed' } });
    fireEvent.keyDown(field(), { key: 'Escape' });
    expect(onClose).toHaveBeenCalledWith('escape', '');
  });

  it('✓ saves without taking the focus on mouse down', () => {
    const { onClose } = setup();
    const ok = screen.getByRole('button', { name: 'Save name' });
    expect(fireEvent.mouseDown(ok)).toBe(false);
    expect(fireEvent.mouseDown(host())).toBe(false);
    expect(fireEvent.mouseDown(field())).toBe(true);
    fireEvent.click(ok);
    expect(onClose).toHaveBeenCalledWith('save', 'Kept in game, 21:04');
  });

  it('losing the focus saves what was typed', () => {
    const { onClose } = setup();
    fireEvent.change(field(), { target: { value: 'Half typ' } });
    fireEvent.blur(field());
    expect(onClose).toHaveBeenCalledWith('blur', 'Half typ');
  });

  it('moving between the field and ✓ isn’t a focus loss; leaving the card is', () => {
    const { onClose } = setup();
    const ok = screen.getByRole('button', { name: 'Save name' });
    fireEvent.blur(field(), { relatedTarget: ok });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(ok, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledWith('escape', '');
  });

  it('the window losing the focus saves what was typed', () => {
    const { onClose } = setup({ place: 'app' });
    fireEvent.change(field(), { target: { value: 'Half typ' } });
    fireEvent.blur(window);
    expect(onClose).toHaveBeenCalledWith('blur', 'Half typ');
  });

  it('the second focus attempt keeps what was typed before it', async () => {
    setup();
    expect(field()).toHaveFocus();
    fireEvent.change(field(), { target: { value: 'B' } });
    field().setSelectionRange(1, 1);
    await advance(30);
    expect(field()).toHaveValue('B');
    expect(field().selectionStart).toBe(1);
  });

  it('closes once, whatever comes after', async () => {
    const { onClose, onDone, rerender } = setup();
    fireEvent.keyDown(field(), { key: 'Enter' });
    fireEvent.blur(field());
    fireEvent.keyDown(field(), { key: 'Escape' });
    rerender(true);
    await advance(2000);
    expect(onClose).toHaveBeenCalledOnce();
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('closes as a focus loss when the host says the game took it back', () => {
    const { onClose, rerender } = setup();
    rerender(true);
    expect(onClose).toHaveBeenCalledWith('blur', 'Kept in game, 21:04');
  });

  it('only folds when there is nowhere to fly, waiting for the answer', async () => {
    let answer!: (to: null) => void;
    const onClose = vi.fn(() => new Promise<null>((resolve) => { answer = resolve; }));
    const { onDone } = setup({ place: 'app', onClose });
    expect(host()).toHaveClass('app');
    fireEvent.keyDown(field(), { key: 'Enter' });
    await advance(1000);
    expect(host()).toHaveClass('closing');
    answer(null);
    await advance(0);
    expect(host()).toHaveClass('folded');
    expect(host()).not.toHaveClass('flying');
    await advance(120);
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('flies onto the row in the app a little longer', async () => {
    const { onDone } = setup({ place: 'app' });
    fireEvent.keyDown(field(), { key: 'Enter' });
    await advance(420 + 440);
    expect(onDone).not.toHaveBeenCalled();
    await advance(20);
    expect(onDone).toHaveBeenCalledOnce();
  });

  it('reduced motion fades out in 200 ms and never flies', async () => {
    const { onDone } = setup({ card: card({ motion: 'reduced' }) });
    expect(host()).toHaveClass('rm');
    fireEvent.keyDown(field(), { key: 'Enter' });
    await advance(200);
    expect(onDone).toHaveBeenCalledOnce();
    expect(host()).not.toHaveClass('flying');
  });
});

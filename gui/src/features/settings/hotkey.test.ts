import { describe, expect, it } from 'vitest';
import { accelerator, keysOf, type KeyPress } from './hotkey';

const press = (key: string, code: string, mods: Partial<KeyPress> = {}): KeyPress => ({
  key, code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods,
});
const ctrl = { ctrlKey: true };

describe('accelerator', () => {
  it('records the modifiers in order, then the key', () => {
    expect(accelerator(press('K', 'KeyK', { ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+K');
    expect(accelerator(press('7', 'Digit7', { altKey: true }))).toBe('Alt+7');
    expect(accelerator(press('F24', 'F24', { shiftKey: true, altKey: true, ctrlKey: true }))).toBe('Ctrl+Alt+Shift+F24');
  });

  it('takes F1–F24 alone, and waits for a key while only modifiers are held', () => {
    expect(accelerator(press('F1', 'F1'))).toBe('F1');
    expect(accelerator(press('F13', 'F13'))).toBe('F13');
    expect(accelerator(press('f', 'KeyF'))).toBeNull();
    expect(accelerator(press('Control', 'ControlLeft', ctrl))).toBeNull();
    expect(accelerator(press('Shift', 'ShiftRight', { shiftKey: true }))).toBeNull();
  });

  // Windows registers the virtual key the layout gives a letter, so the letter typed is the one stored.
  it('records the letter the layout types, not where the key sits', () => {
    // AZERTY: A sits where QWERTY has Q, and Z where it has W.
    expect(accelerator(press('a', 'KeyQ', ctrl))).toBe('Ctrl+A');
    expect(accelerator(press('z', 'KeyW', ctrl))).toBe('Ctrl+Z');
    // QWERTZ: Z and Y swap.
    expect(accelerator(press('z', 'KeyY', ctrl))).toBe('Ctrl+Z');
    expect(accelerator(press('Y', 'KeyZ', { ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+Y');
    // Dvorak: S is typed by the key QWERTY has as ;.
    expect(accelerator(press('s', 'Semicolon', { altKey: true }))).toBe('Alt+S');
  });

  it('falls back to where the key sits when it types no A–Z letter', () => {
    // Russian: Windows gives that key the K virtual key, as on QWERTY.
    expect(accelerator(press('л', 'KeyK', ctrl))).toBe('Ctrl+K');
    // AltGr (Ctrl+Alt) on QWERTZ types € on E.
    expect(accelerator(press('€', 'KeyE', { ctrlKey: true, altKey: true }))).toBe('Ctrl+Alt+E');
    // Spanish ñ isn't a key we take.
    expect(accelerator(press('ñ', 'Semicolon', ctrl))).toBeNull();
  });

  it('records the number row as its digit, shifted or not, on any layout', () => {
    // US: Shift+1 types !.
    expect(accelerator(press('!', 'Digit1', { ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+1');
    // AZERTY types & unshifted and 1 shifted on the same key.
    expect(accelerator(press('&', 'Digit1', ctrl))).toBe('Ctrl+1');
    expect(accelerator(press('1', 'Digit1', { ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+1');
    expect(accelerator(press('à', 'Digit0', { altKey: true }))).toBe('Alt+0');
  });

  it('refuses F12, bare keys, the Windows key and keys outside A–Z, 0–9 and F1–F24', () => {
    expect(accelerator(press('F12', 'F12'))).toBeNull();
    expect(accelerator(press('F12', 'F12', ctrl))).toBeNull();
    expect(accelerator(press('k', 'KeyK'))).toBeNull();
    expect(accelerator(press('&', 'Digit1'))).toBeNull();
    expect(accelerator(press('k', 'KeyK', { metaKey: true, ctrlKey: true }))).toBeNull();
    expect(accelerator(press(' ', 'Space', ctrl))).toBeNull();
    expect(accelerator(press('1', 'Numpad1', ctrl))).toBeNull();
    expect(accelerator(press('F25', 'F25', ctrl))).toBeNull();
  });
});

describe('keysOf', () => {
  it('splits a stored accelerator into its key caps, none when off', () => {
    expect(keysOf('Ctrl+Shift+K')).toEqual(['Ctrl', 'Shift', 'K']);
    expect(keysOf('F9')).toEqual(['F9']);
    expect(keysOf('')).toEqual([]);
  });
});

// The overlay hotkeys' stored form, the one Rust registers (`overlay/hotkeys.rs`): `Ctrl`, `Alt`,
// `Shift` in that order, then A–Z, 0–9 or F1–F24. `''` is off.

/** What a key press records. */
export interface KeyPress {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** The key Windows registers for `press`, which names keys as the layout does, not by position:
 *  the plugin turns a stored `A` into the A virtual key, so an AZERTY player's A is `A` though it
 *  sits where QWERTY has Q. A letter is the one the key types; a key that types no A–Z letter (a
 *  Cyrillic layout, AltGr) falls back to its position. Digits are the number row, shifted or not
 *  (AZERTY's types `&` unshifted), and F-keys are themselves. */
function keyOf(press: KeyPress): string | undefined {
  const digit = /^Digit([0-9])$/.exec(press.code)?.[1];
  if (digit !== undefined) return digit;
  if (/^[a-z]$/i.test(press.key)) return press.key.toUpperCase();
  const letter = /^Key([A-Z])$/.exec(press.code)?.[1];
  if (letter !== undefined) return letter;
  return /^F([1-9]|1[0-9]|2[0-4])$/.test(press.code) ? press.code : undefined;
}

/** The accelerator `press` records, or `null` while it's a modifier alone or a key we refuse:
 *  F1–F24 may go alone, the rest need a modifier, and never F12 (Windows keeps it for debuggers). */
export function accelerator(press: KeyPress): string | null {
  if (press.metaKey) return null;
  const key = keyOf(press);
  if (key === undefined || key === 'F12') return null;
  const mods = [press.ctrlKey && 'Ctrl', press.altKey && 'Alt', press.shiftKey && 'Shift'].filter((m) => m !== false);
  if (mods.length === 0 && !/^F\d/.test(key)) return null;
  return [...mods, key].join('+');
}

/** The key caps a stored accelerator shows, none when it's off. */
export function keysOf(stored: string): string[] {
  return stored === '' ? [] : stored.split('+');
}

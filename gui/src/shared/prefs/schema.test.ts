import { describe, expect, it } from 'vitest';
import { DEFAULT_PREFS, PREFS_VERSION, parsePrefs, serializePrefs } from './schema';

describe('parsePrefs', () => {
  it('returns the defaults for a missing or malformed file', () => {
    expect(parsePrefs(undefined)).toEqual(DEFAULT_PREFS);
    expect(parsePrefs(null)).toEqual(DEFAULT_PREFS);
    expect(parsePrefs('nonsense')).toEqual(DEFAULT_PREFS);
  });

  it('keeps valid values and replaces invalid ones field by field', () => {
    const prefs = parsePrefs({
      version: PREFS_VERSION,
      language: 'es',
      tone: 'neon',
      motion: 'reduced',
      onboardingSeen: 'yes',
      hints: { secondDevice: true, broken: 1 },
    });
    expect(prefs).toEqual({
      ...DEFAULT_PREFS,
      language: 'es',
      motion: 'reduced',
      hints: { secondDevice: true },
    });
  });

  it('round-trips through serializePrefs with the schema version', () => {
    const prefs = { ...DEFAULT_PREFS, tone: 'warm' as const, onboardingSeen: true };
    const stored = serializePrefs(prefs);
    expect(stored.version).toBe(PREFS_VERSION);
    expect(parsePrefs(JSON.parse(JSON.stringify(stored)))).toEqual(prefs);
  });

  it('keeps the scan view and defaults older or invalid preferences to grid', () => {
    expect(parsePrefs({ scanView: 'list' }).scanView).toBe('list');
    expect(parsePrefs({}).scanView).toBe('grid');
    expect(parsePrefs({ scanView: 'table' }).scanView).toBe('grid');
  });
});

describe('parsePrefs overlay', () => {
  it('defaults to a popup with sound in the bottom right and the two hotkeys', () => {
    expect(parsePrefs({}).overlay).toEqual({
      enabled: true,
      sound: true,
      corner: 'br',
      perGame: {},
      quiet: false,
      warnAhead: true,
      quickKeepKey: 'Ctrl+Shift+K',
      nameItKey: 'Ctrl+Shift+N',
    });
  });

  it('keeps valid values and replaces invalid ones field by field', () => {
    const overlay = parsePrefs({
      overlay: {
        enabled: false,
        sound: 'loud',
        corner: 'centre',
        perGame: { a: 'tl', b: 'off', c: 'middle', d: 3 },
        quiet: true,
        warnAhead: null,
        quickKeepKey: '',
        nameItKey: 42,
      },
    }).overlay;
    expect(overlay).toEqual({
      ...DEFAULT_PREFS.overlay,
      enabled: false,
      perGame: { a: 'tl', b: 'off' },
      quiet: true,
      // A turned-off hotkey is an empty shortcut, not a missing one.
      quickKeepKey: '',
    });
  });

  it('falls back to the defaults when the overlay or its per-game map is malformed', () => {
    expect(parsePrefs({ overlay: 'on' }).overlay).toEqual(DEFAULT_PREFS.overlay);
    expect(parsePrefs({ overlay: { perGame: ['tl'], corner: 'tr' } }).overlay).toEqual({ ...DEFAULT_PREFS.overlay, corner: 'tr' });
  });

  it('round-trips through serializePrefs', () => {
    const prefs = { ...DEFAULT_PREFS, overlay: { ...DEFAULT_PREFS.overlay, corner: 'tl' as const, perGame: { uuid: 'off' as const } } };
    expect(parsePrefs(JSON.parse(JSON.stringify(serializePrefs(prefs))))).toEqual(prefs);
  });
});

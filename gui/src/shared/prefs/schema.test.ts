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
});

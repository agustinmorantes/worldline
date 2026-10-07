import { describe, it, expect } from 'vitest';
import { presetOf, RETENTION_PRESETS } from './retention';

describe('retention settings', () => {
  it('recognises presets and treats anything else as custom', () => {
    expect(presetOf(RETENTION_PRESETS.balanced)).toBe('balanced');
    expect(presetOf(RETENTION_PRESETS.sparse)).toBe('sparse');
    expect(presetOf({ ...RETENTION_PRESETS.dense, weekly_until_days: 365 })).toBe('custom');
  });
});

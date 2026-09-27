import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { getPruningSettings, presetOf, RETENTION_PRESETS } from './pruning';

describe('retention settings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('recognises presets and treats anything else as custom', () => {
    expect(presetOf(RETENTION_PRESETS.balanced)).toBe('balanced');
    expect(presetOf(RETENTION_PRESETS.sparse)).toBe('sparse');
    expect(presetOf({ ...RETENTION_PRESETS.dense, weekly_until_days: 365 })).toBe('custom');
  });

  it('parses optional budget and branch expiry', async () => {
    vi.mocked(invoke).mockResolvedValue({
      automatic: true,
      ...RETENTION_PRESETS.balanced,
      branch_expiry_days: null,
      max_bytes: 1048576,
      updated_at: '2026-09-27T00:00:00Z',
    });
    const settings = await getPruningSettings();
    expect(settings.branch_expiry_days).toBeNull();
    expect(settings.max_bytes).toBe(1048576);
  });
});

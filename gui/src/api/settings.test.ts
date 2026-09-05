import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { updateMonitoringSettings, getMonitoringSettings } from './settings';

describe('settings API', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('passes monitoringSettings as camelCase key when updating settings', async () => {
    // Regression: the original bug used snake_case `monitoring_settings` as the
    // invoke key, which Tauri v2 could not match to the Rust parameter name.
    const settings = { version_cooldown_minutes: 30, snapshot_debounce_seconds: 10 };
    vi.mocked(invoke).mockResolvedValue(settings);

    await updateMonitoringSettings(settings);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      'update_monitoring_settings',
      { monitoringSettings: settings },
    );
  });

  it('fetches monitoring settings without extra arguments', async () => {
    const settings = { version_cooldown_minutes: 15, snapshot_debounce_seconds: 5 };
    vi.mocked(invoke).mockResolvedValue(settings);

    const result = await getMonitoringSettings();

    expect(vi.mocked(invoke)).toHaveBeenCalledWith('get_monitoring_settings');
    expect(result).toEqual(settings);
  });
});

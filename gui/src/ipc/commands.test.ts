import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import * as commands from './commands';

// Tauri maps camelCase argument names to the Rust parameter names; a wrong name
// fails only at runtime, so the ones that have broken before are pinned here.
describe('ipc commands', () => {
  const monitoring = { version_cooldown_minutes: 5, snapshot_debounce_seconds: 3 };

  beforeEach(() => {
    vi.mocked(invoke).mockReset().mockResolvedValue(null);
  });

  it.each([
    [
      'update_monitoring_settings',
      () => commands.updateMonitoringSettings(monitoring),
      { monitoringSettings: monitoring },
    ],
    ['save_game', () => commands.saveGame({ id: null, title: 'Celeste' }, ['C:/saves']), { game: { id: null, title: 'Celeste' }, dirs: ['C:/saves'] }],
    ['delete_slot', () => commands.deleteSlot(2, 1), { slotId: 2, replacementSlotId: 1 }],
    ['switch_slot', () => commands.switchSlot(1, 2), { gameId: 1, slotId: 2 }],
    ['set_game_icon', () => commands.setGameIcon(1, 'C:/icon.png'), { gameId: 1, sourcePath: 'C:/icon.png' }],
    ['apply_remote_version', () => commands.applyRemoteVersion(7), { versionId: 7 }],
    ['adopt_remote_game', () => commands.adoptRemoteGame('uuid'), { gameUuid: 'uuid' }],
    [
      'import_game_history',
      () => commands.importGameHistory('C:/bundle', { dir: 'C:/saves' }),
      { bundlePath: 'C:/bundle', mappings: { dir: 'C:/saves' } },
    ],
    ['pick_save_path', () => commands.pickSavePath('Export', 'history'), { title: 'Export', fileName: 'history' }],
  ])('%s', async (command, call, args) => {
    await call();
    expect(invoke).toHaveBeenCalledWith(command, args);
  });
});

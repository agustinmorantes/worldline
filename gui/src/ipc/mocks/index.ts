// Replaces the Tauri backend with fixtures so the UI runs on plain `vite dev`
// (`bun run dev:mock`, Playwright screen tests). Never bundled into the app.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { InvokeArgs } from '@tauri-apps/api/core';
import { FIXTURES, type Fixture, type FixtureName } from './fixtures';

const FIXTURE_KEY = 'worldline-mock-fixture';
const STORE_KEY = 'worldline-mock-store';

function fixtureName(): FixtureName {
  const name = localStorage.getItem(FIXTURE_KEY);
  return name !== null && name in FIXTURES ? (name as FixtureName) : 'default';
}

type Args = Record<string, unknown>;

/** tauri-plugin-store, backed by localStorage so tests can seed preferences. */
function storeCommand(cmd: string, args: Args): unknown {
  const values = JSON.parse(localStorage.getItem(STORE_KEY) ?? '{}') as Record<string, unknown>;
  switch (cmd) {
    case 'load':
      return 1;
    case 'get':
      return [values[args.key as string], args.key as string in values];
    case 'set':
      localStorage.setItem(STORE_KEY, JSON.stringify({ ...values, [args.key as string]: args.value }));
      return null;
    case 'save':
      return null;
    default:
      throw new Error(`No mock for plugin:store|${cmd}`);
  }
}

function backendCommand(state: Fixture, cmd: string, args: Args): unknown {
  const game = (id: unknown): Fixture['games'][number] => {
    const found = state.games.find((g) => g.id === id);
    if (found === undefined) throw new Error(`Game ${String(id)} not found`);
    return found;
  };

  switch (cmd) {
    case 'get_games':
      return state.games;
    case 'get_game':
      return game(args.id);
    case 'get_prefs_path':
      return 'mock://prefs.json';
    case 'get_startup_notice':
      return null;
    case 'get_device':
      return state.device;
    case 'get_sync_settings':
      return state.syncSettings;
    case 'refresh_sync':
      if (state.syncReport === 'pending') return new Promise(() => undefined);
      if (state.syncReport === undefined) throw new Error('Sync is not configured');
      return state.syncReport;
    case 'get_retired_versions':
    case 'get_scan_roots':
    case 'scan_installed':
      return [];
    case 'get_monitoring_settings':
      return state.monitoringSettings;
    case 'get_pruning_settings':
      return state.retentionSettings;
    case 'preview_pruning':
      return {
        retire_versions: [],
        live_bytes_before: 0,
        live_bytes_after: 0,
        budget_unreachable: false,
        local_gc: { chunks_deleted: 0, bytes_deleted: 0, chunks_condemned: 0 },
      };
    case 'pick_folder':
    case 'pick_file':
    case 'pick_save_path':
      return null;
    case 'confirm':
      return true;
    default:
      throw new Error(`No mock for command ${cmd}`);
  }
}

export function installMocks(): void {
  const state = FIXTURES[fixtureName()]();
  mockWindows('main');
  mockIPC(
    (cmd: string, payload?: InvokeArgs) => {
      const args = (payload ?? {}) as Args;
      if (cmd.startsWith('plugin:store|')) return storeCommand(cmd.slice('plugin:store|'.length), args);
      if (cmd === 'plugin:log|log') return null;
      if (cmd.startsWith('plugin:window|')) return null;
      return backendCommand(state, cmd, args);
    },
    { shouldMockEvents: true },
  );
}

// Replaces the Tauri backend with fixtures so the UI runs on plain `vite dev`
// (`bun run dev:mock`, Playwright screen tests). Never bundled into the app.
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import type { InvokeArgs } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { makeGame, makeSlot, makeTrackedDir } from '../builders';
import type { DeviceKind } from '../bindings/DeviceKind';
import type { Failure } from '../bindings/Failure';
import type { FullGame } from '../bindings/FullGame';
import type { MonitoringSettings } from '../bindings/MonitoringSettings';
import type { PruningPreview } from '../bindings/PruningPreview';
import type { RetentionSettings } from '../bindings/RetentionSettings';
import type { SyncSettings } from '../bindings/SyncSettings';
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

/** A made-up estimate that grows as the tiers shrink, so it moves with the steppers. */
function previewOf(settings: RetentionSettings): PruningPreview {
  const kept = Math.min(1, (settings.keep_all_hours + settings.hourly_until_hours / 4 + settings.daily_until_days * 2 + settings.weekly_until_days / 2) / 400);
  const before = 4_000_000_000;
  return {
    retire_versions: [],
    live_bytes_before: before,
    live_bytes_after: Math.round(before * kept),
    budget_unreachable: false,
    local_gc: { chunks_deleted: 0, bytes_deleted: 0, chunks_condemned: 0 },
  };
}

function backendCommand(state: Fixture, cmd: string, args: Args): unknown {
  const game = (id: unknown): Fixture['games'][number] => {
    const found = state.games.find((g) => g.id === id);
    if (found === undefined) throw new Error(`Game ${String(id)} not found`);
    return found;
  };

  switch (cmd) {
    case 'get_running_games':
      return [...(state.running ?? [])];
    case 'get_games':
      return structuredClone(state.games);
    case 'get_game':
      return structuredClone(game(args.id));
    case 'restore_version':
    case 'apply_remote_version': {
      const versionId = cmd === 'restore_version' ? args.id : args.versionId;
      const slot = state.games.find((g) => g.versions.some((v) => v.id === versionId))?.active_slot ?? null;
      if (slot === null) throw new Error(`Version ${String(versionId)} not found`);
      slot.active_version_id = versionId as number;
      return null;
    }
    case 'tag_version': {
      const version = state.games.flatMap((g) => g.versions).find((v) => v.id === args.id);
      if (version === undefined) throw new Error(`Version ${String(args.id)} not found`);
      version.tag = args.tag === '' ? null : (args.tag as string);
      return version;
    }
    case 'switch_slot': {
      const target = game(args.gameId);
      const slot = target.slots.find((s) => s.id === args.slotId);
      if (slot === undefined) throw new Error(`Slot ${String(args.slotId)} not found`);
      target.active_slot = slot;
      target.active_slot_id = slot.id;
      return target;
    }
    case 'create_slot': {
      const target = game(args.gameId);
      const id = Math.max(0, ...state.games.flatMap((g) => g.slots.map((s) => s.id))) + 1;
      const slot = makeSlot({ id, game_id: target.id, name: args.name as string, active_version_id: null });
      target.slots.push(slot);
      target.slot_summaries.push({ slot_id: id, version_count: 0, last_created_at: null });
      target.active_slot = slot;
      target.active_slot_id = id;
      return target;
    }
    case 'save_game': {
      const form = args.game as { id: number | null; title: string };
      const dirs = args.dirs as string[] | null;
      const ids = state.games.flatMap((g) => g.tracked_dirs.map((d) => d.id));
      const tracked = (gameId: number): FullGame['tracked_dirs'] =>
        (dirs ?? []).map((path, i) => makeTrackedDir({ id: Math.max(0, ...ids) + i + 1, game_id: gameId, path }));
      if (form.id === null) {
        const id = Math.max(0, ...state.games.map((g) => g.id)) + 1;
        const created = makeGame({ id, title: form.title, tracked_dirs: tracked(id) });
        state.games.push(created);
        return structuredClone(created);
      }
      const target = game(form.id);
      target.title = form.title;
      if (dirs !== null) target.tracked_dirs = tracked(target.id);
      return structuredClone(target);
    }
    case 'set_game_icon':
      return null;
    case 'set_game_accent': {
      const target = game(args.gameId);
      const hue = args.hue as number | null;
      target.accent_hue = hue ?? 290;
      target.accent_hue_source = hue === null ? 'auto' : 'user';
      return structuredClone(target);
    }
    case 'delete_game':
    case 'untrack_game':
      state.games = state.games.filter((g) => g.id !== args.id);
      return null;
    case 'adopt_remote_game': {
      const report = state.syncReport;
      const remote = typeof report === 'object' ? report.remote_games.find((g) => g.uuid === args.gameUuid) : undefined;
      if (remote === undefined) throw new Error(`Remote game ${String(args.gameUuid)} not found`);
      remote.adopted = true;
      const id = Math.max(0, ...state.games.map((g) => g.id)) + 1;
      const adopted = makeGame({ id, title: remote.title, uuid: remote.uuid });
      state.games.push(adopted);
      return structuredClone(adopted);
    }
    case 'scan_installed':
      return state.scan === 'pending' ? new Promise(() => undefined) : structuredClone(state.scan ?? []);
    case 'get_scan_roots':
      return [...(state.scanRoots ?? [])];
    case 'add_scan_root':
      state.scanRoots = [...(state.scanRoots ?? []), args.path as string];
      return null;
    case 'remove_scan_root':
      state.scanRoots = (state.scanRoots ?? []).filter((r) => r !== args.path);
      return null;
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
      if (state.syncReport === 'unreachable') {
        const failure: Failure = { message: 'Sync folder is unavailable: The system cannot find the path specified. (os error 3)', kind: 'sync_folder_unreachable' };
        // Tauri rejects with the command's error value as is, not an Error.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject(failure);
      }
      if (state.syncReport === undefined) throw new Error('Sync is not configured');
      return state.syncReport;
    case 'get_retired_versions':
      return [];
    case 'update_device':
      state.device = { ...state.device, name: args.name as string, kind: args.kind as DeviceKind | null };
      return state.device;
    case 'update_sync_settings':
      state.syncSettings = args.settings as SyncSettings;
      // A fresh cloud folder knows only this device.
      state.syncReport ??= {
        synced_at: new Date().toISOString(),
        games: [],
        remote_games: [],
        devices: [state.device],
        remote_versions: {},
        problems: [],
      };
      return state.syncSettings;
    case 'remove_sync_device':
      if (typeof state.syncReport === 'object') {
        state.syncReport.devices = state.syncReport.devices.filter((d) => d.device_uuid !== args.deviceUuid);
      }
      return null;
    case 'get_monitoring_settings':
      return state.monitoringSettings;
    case 'update_monitoring_settings':
      state.monitoringSettings = args.monitoringSettings as MonitoringSettings;
      return state.monitoringSettings;
    case 'get_pruning_settings':
      return state.retentionSettings;
    case 'update_pruning_settings':
      state.retentionSettings = args.settings as RetentionSettings;
      return state.retentionSettings;
    case 'preview_pruning':
      return previewOf((args.settings as RetentionSettings | null) ?? state.retentionSettings);
    case 'run_pruning': {
      const preview = previewOf(state.retentionSettings);
      return {
        retired_versions: [],
        budget_unreachable: false,
        local_gc: { chunks_deleted: 12, bytes_deleted: preview.live_bytes_before - preview.live_bytes_after, chunks_condemned: 0 },
        remote_gc: null,
      };
    }
    case 'set_tray_labels':
      return null;
    case 'pick_folder':
      return state.pickedFolder ?? null;
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
      if (cmd.startsWith('plugin:window|') || cmd.startsWith('plugin:webview|')) return null;
      return backendCommand(state, cmd, args);
    },
    { shouldMockEvents: true },
  );
  // Screen tests play backend events (a game stopping) through this.
  (window as unknown as { mockEmit: typeof emit }).mockEmit = emit;
}

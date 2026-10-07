// Backend states for the mocked `vite dev` UI (`bun run dev:mock`) and the Playwright
// screen tests. Pick one with `localStorage['worldline-mock-fixture']`.
import { makeGame, makeSlot, makeTrackedDir, makeVersion, naive } from '../builders';
import type { DeviceMetadata } from '../bindings/DeviceMetadata';
import type { FullGame } from '../bindings/FullGame';
import type { MonitoringSettings } from '../bindings/MonitoringSettings';
import type { RetentionSettings } from '../bindings/RetentionSettings';
import type { SyncSettings } from '../bindings/SyncSettings';

export interface Fixture {
  games: FullGame[];
  device: DeviceMetadata;
  syncSettings: SyncSettings;
  monitoringSettings: MonitoringSettings;
  retentionSettings: RetentionSettings;
}

const minutesAgo = (minutes: number): string => naive(new Date(Date.now() - minutes * 60_000));

function eldenRing(): FullGame {
  const main = makeSlot({ id: 1, game_id: 1, name: 'Main', active_version_id: 3 });
  const ng = makeSlot({ id: 2, game_id: 1, name: 'NG+', active_version_id: 4 });
  const versions = [
    makeVersion({ id: 3, slot_id: 1, parent_id: 2, version_number: 3, tag: 'Before Malenia', created_at: minutesAgo(12) }),
    makeVersion({ id: 2, slot_id: 1, parent_id: 1, version_number: 2, created_at: minutesAgo(95) }),
    makeVersion({ id: 1, slot_id: 1, version_number: 1, created_at: minutesAgo(60 * 26) }),
  ];
  return makeGame({
    id: 1,
    title: 'Elden Ring',
    active_slot_id: 1,
    active_slot: main,
    slots: [main, ng],
    versions,
    last_version: versions[0],
    tracked_dirs: [makeTrackedDir({ id: 1, game_id: 1, path: 'C:/Users/Player/AppData/Roaming/EldenRing' })],
  });
}

function hollowKnight(): FullGame {
  const main = makeSlot({ id: 3, game_id: 2, name: 'Main', active_version_id: 5 });
  const version = makeVersion({ id: 5, slot_id: 3, version_number: 1, created_at: minutesAgo(60 * 24 * 3) });
  return makeGame({
    id: 2,
    title: 'Hollow Knight',
    active_slot_id: 3,
    active_slot: main,
    slots: [main],
    versions: [version],
    last_version: version,
    tracked_dirs: [makeTrackedDir({ id: 2, game_id: 2, path: 'C:/Users/Player/AppData/LocalLow/Team Cherry/Hollow Knight' })],
  });
}

function base(games: FullGame[]): Fixture {
  return {
    games,
    device: {
      device_uuid: '00000000-0000-4000-8000-00000000d001',
      name: 'Desktop',
      platform: 'windows',
      first_seen: '2024-01-01T00:00:00Z',
      last_seen: '2024-01-01T00:00:00Z',
    },
    syncSettings: { remote_path: null },
    monitoringSettings: { version_cooldown_minutes: 5, snapshot_debounce_seconds: 3 },
    retentionSettings: {
      automatic: true,
      keep_all_hours: 1,
      hourly_until_hours: 24,
      daily_until_days: 7,
      weekly_until_days: 90,
      branch_expiry_days: null,
      max_bytes: null,
      updated_at: '2024-01-01T00:00:00Z',
    },
  };
}

export const FIXTURES = {
  default: () => base([eldenRing(), hollowKnight()]),
  empty: () => base([]),
} satisfies Record<string, () => Fixture>;

export type FixtureName = keyof typeof FIXTURES;

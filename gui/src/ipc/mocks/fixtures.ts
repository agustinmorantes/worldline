// Backend states for the mocked `vite dev` UI (`bun run dev:mock`) and the Playwright
// screen tests. Pick one with `localStorage['worldline-mock-fixture']`.
import { makeGame, makeSlot, makeTrackedDir, makeVersion, naive } from '../builders';
import type { DeviceMetadata } from '../bindings/DeviceMetadata';
import type { Divergence } from '../bindings/Divergence';
import type { FullGame } from '../bindings/FullGame';
import type { GameSyncStatus } from '../bindings/GameSyncStatus';
import type { InstallScannerItem } from '../bindings/InstallScannerItem';
import type { MonitoringSettings } from '../bindings/MonitoringSettings';
import type { RetentionSettings } from '../bindings/RetentionSettings';
import type { SyncReport } from '../bindings/SyncReport';
import type { SyncSettings } from '../bindings/SyncSettings';

export interface Fixture {
  games: FullGame[];
  device: DeviceMetadata;
  syncSettings: SyncSettings;
  monitoringSettings: MonitoringSettings;
  retentionSettings: RetentionSettings;
  /** What `refresh_sync` answers; `'pending'` never settles, `'unreachable'` can't find the folder. Unset: sync fails. */
  syncReport?: SyncReport | 'pending' | 'unreachable';
  /** What `scan_installed` answers; `'pending'` never settles. */
  scan?: InstallScannerItem[] | 'pending';
  scanRoots?: string[];
  /** What `pick_folder` answers; unset, the picker is cancelled. */
  pickedFolder?: string;
  /** Shortcuts `check_shortcut` refuses, as if another app held them. */
  takenShortcuts?: string[];
  /** What `get_running_games` answers: ids of the games running now. */
  running?: number[];
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
      kind: 'desktop',
    },
    syncSettings: { remote_path: null },
    monitoringSettings: { version_cooldown_minutes: 5, snapshot_debounce_seconds: 3, screenshots_enabled: false, sync_screenshots: true },
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

function savedGame(id: number, title: string, minutes: number): FullGame {
  const main = makeSlot({ id: 100 + id, game_id: id, name: 'Main', active_version_id: 100 + id });
  const version = makeVersion({ id: 100 + id, slot_id: 100 + id, version_number: 1, created_at: minutesAgo(minutes) });
  return makeGame({
    id,
    title,
    active_slot_id: main.id,
    active_slot: main,
    slots: [main],
    versions: [version],
    last_version: version,
  });
}

/** Several games with sync on: one has news from the handheld, one cloud game isn't here. */
function library(): Fixture {
  const games = [eldenRing(), hollowKnight(), savedGame(3, 'Hades', 60 * 5), savedGame(4, 'Celeste', 60 * 24 * 9), savedGame(5, 'Stardew Valley', 60 * 24 * 30)];
  const status = (game: FullGame, relation: Divergence, device: string | null = null): GameSyncStatus => ({
    game_uuid: game.uuid,
    relation,
    remote_device: device,
    remote_version_uuid: null,
    remote_version_id: null,
    auto_applied: false,
  });
  const state = base(games);
  return {
    ...state,
    syncSettings: { remote_path: 'D:/Cloud/Worldline' },
    syncReport: {
      synced_at: new Date(Date.now() - 4 * 60_000).toISOString(),
      games: [status(games[0], 'in_sync'), status(games[1], 'fast_forward', 'Steam Deck'), status(games[2], 'in_sync'), status(games[3], 'in_sync')],
      remote_games: [{ uuid: 'remote-1', title: 'Hollow Knight: Silksong', installed: false, adopted: false, source_device: 'Steam Deck', version_count: 4 }],
      devices: [],
      remote_versions: {},
      problems: [],
    },
  };
}


const DECK: DeviceMetadata = {
  device_uuid: '00000000-0000-4000-8000-00000000d002',
  name: 'Steam Deck',
  platform: 'linux',
  first_seen: '2024-01-01T00:00:00Z',
  last_seen: '2024-01-01T00:00:00Z',
  kind: 'handheld',
};

type Checkpoint = [id: number, parent: number | null, minutesAgo: number, tag?: string | null, deck?: boolean];

/** Three days of Hades: a session on the handheld, a timeline you left, and tonight's play. */
const HADES: Checkpoint[] = [
  [1, null, 60 * 72 + 60, null, true],
  [2, 1, 60 * 72 + 25, 'Asphodel', true],
  [3, 2, 60 * 26],
  [4, 3, 60 * 26 - 25],
  [5, 4, 60 * 26 - 50, 'Before Hydra'],
  [6, 4, 60 * 24],
  [7, 6, 60 * 24 - 30],
  [8, 5, 40],
  [9, 8, 25, 'Elysium'],
  [10, 9, 8],
];

function hades(extra: Checkpoint[] = []): FullGame {
  const main = makeSlot({ id: 1, game_id: 1, name: 'Main', active_version_id: 10 });
  const versions = [...HADES, ...extra].map(([id, parent, minutes, tag = null, deck = false]) => makeVersion({
    id,
    slot_id: 1,
    parent_id: parent,
    version_number: id,
    tag,
    created_at: minutesAgo(minutes),
    origin_device: deck ? DECK.device_uuid : null,
  }));
  return makeGame({
    id: 1,
    title: 'Hades',
    active_slot_id: 1,
    active_slot: main,
    slots: [main],
    slot_summaries: [{ slot_id: 1, version_count: versions.length, last_created_at: versions[versions.length - 1].created_at }],
    versions,
    last_version: versions[versions.length - 1],
    tracked_dirs: [makeTrackedDir({ id: 1, game_id: 1, path: 'C:/Users/Player/Saved Games/Hades' })],
  });
}

function celeste(): FullGame {
  const main = makeSlot({ id: 2, game_id: 2, name: 'Main', active_version_id: null });
  return makeGame({ id: 2, title: 'Celeste', active_slot_id: 2, active_slot: main, slots: [main], versions: [], last_version: null });
}

/** The Sessions view: Hades with history, Celeste with none, the handheld synced in. */
function sessions(relation: Divergence = 'in_sync', extra: Checkpoint[] = []): Fixture {
  const games = [hades(extra), celeste()];
  const deckVersions = games[0].versions.filter((v) => v.origin_device === DECK.device_uuid);
  const pending = relation === 'fast_forward' ? deckVersions[deckVersions.length - 1] : null;
  return {
    ...base(games),
    syncSettings: { remote_path: 'D:/Cloud/Worldline' },
    syncReport: {
      synced_at: new Date(Date.now() - 2 * 60_000).toISOString(),
      games: [{
        game_uuid: games[0].uuid,
        relation,
        remote_device: relation === 'in_sync' ? null : DECK.name,
        remote_version_uuid: pending?.uuid ?? null,
        remote_version_id: pending?.id ?? null,
        auto_applied: false,
      }],
      remote_games: [],
      devices: [DECK],
      remote_versions: Object.fromEntries(deckVersions.map((v) => [v.uuid, DECK.name])),
      problems: [],
    },
  };
}


/** The Add games view: three games in the cloud and a scan that finds a few more. */
function addGames(scan: Fixture['scan']): Fixture {
  const state = library();
  const report = state.syncReport as SyncReport;
  return {
    ...state,
    syncReport: {
      ...report,
      devices: [DECK, { ...DECK, device_uuid: '00000000-0000-4000-8000-00000000d003', name: 'Laptop', platform: 'windows', kind: 'laptop' }],
      remote_games: [
        ...report.remote_games,
        { uuid: 'remote-2', title: 'Balatro', installed: true, adopted: false, source_device: 'Laptop', version_count: 12 },
        { uuid: 'remote-3', title: 'Dave the Diver', installed: false, adopted: false, source_device: 'Steam Deck', version_count: 1 },
        { uuid: 'remote-4', title: 'Tunic', installed: false, adopted: true, source_device: 'Laptop', version_count: 7 },
      ],
    },
    scanRoots: ['D:/Games', 'E:/Emulation/Saves'],
    scan,
  };
}

const SCANNED: InstallScannerItem[] = [
  { title: 'Hades', dirs: ['C:/Users/Player/Saved Games/Hades'], candidate_dirs: [], install_path: null, icon_path: null, last_modified_at: Date.now() - 3_600_000, source: 'steam' },
  { title: 'Sea of Stars', dirs: ['C:/Users/Player/AppData/LocalLow/Sabotage/SeaOfStars'], candidate_dirs: [], install_path: null, icon_path: null, last_modified_at: Date.now() - 7_200_000, source: 'steam' },
  { title: 'Outer Wilds', dirs: ['C:/Users/Player/AppData/LocalLow/Mobius Digital/Outer Wilds'], candidate_dirs: [], install_path: null, icon_path: null, last_modified_at: Date.now() - 86_400_000, source: 'epic' },
  { title: 'Cassette Beasts', dirs: ['D:/Games/Cassette Beasts/saves'], candidate_dirs: [], install_path: null, icon_path: null, last_modified_at: null, source: 'custom' },
];

/** Everything in the cloud is already here, and the scan finds nothing new. */
function addGamesDone(): Fixture {
  const state = addGames([SCANNED[0]]);
  const report = state.syncReport as SyncReport;
  return { ...state, syncReport: { ...report, remote_games: report.remote_games.map((g) => ({ ...g, adopted: true })) } };
}

/** Settings with sync on: this PC, the handheld and a laptop share the cloud. */
function settings(): Fixture {
  const state = library();
  const report = state.syncReport as SyncReport;
  return {
    ...state,
    syncReport: {
      ...report,
      devices: [
        state.device,
        { ...DECK, last_seen: new Date(Date.now() - 120 * 60_000).toISOString() },
        { ...DECK, device_uuid: '00000000-0000-4000-8000-00000000d003', name: 'Old laptop', platform: 'windows', kind: 'laptop', last_seen: new Date(Date.now() - 40 * 86_400_000).toISOString() },
      ],
    },
    pickedFolder: 'E:/OneDrive/Worldline',
  };
}

export const FIXTURES = {
  default: () => base([eldenRing(), hollowKnight()]),
  empty: () => base([]),
  library,
  syncing: () => ({ ...library(), syncReport: 'pending' as const }),
  /** Sync on, but the cloud folder is gone (an unplugged drive, the cloud app closed). */
  offline: () => ({ ...settings(), syncReport: 'unreachable' as const }),
  /** Sync on, but syncing fails for another reason than the folder. */
  syncFailed: () => ({ ...library(), syncReport: undefined }),
  sessions: () => sessions(),
  ghost: () => sessions('fast_forward', [[11, 10, 4, null, true], [12, 11, 2, 'Styx', true]]),
  diverged: () => sessions('diverged', [[11, 9, 6, null, true], [12, 11, 3, 'Styx', true]]),
  addGames: () => addGames(undefined),
  scanning: () => addGames('pending'),
  scanned: () => addGames(SCANNED),
  scannedMany: () => addGames(Array.from({ length: 60 }, (_, i) => ({
    ...SCANNED[1], title: `Game ${i.toString().padStart(2, '0')}`, dirs: [`C:/saves/game-${i.toString()}`],
  }))),
  addGamesDone,
  settings,
  /** Sync off; the folder picker answers with a cloud folder. */
  cloudOff: () => ({ ...base([eldenRing(), hollowKnight()]), pickedFolder: 'D:/Dropbox/Worldline' }),
  /** Elden Ring is running; its row and header say "Recording". */
  running: () => ({ ...base([eldenRing(), hollowKnight()]), running: [1] }),
} satisfies Record<string, () => Fixture>;

export type FixtureName = keyof typeof FIXTURES;

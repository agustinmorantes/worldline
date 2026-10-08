import { describe, expect, it } from 'vitest';
import { makeVersion } from '@/ipc/builders';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { GameSyncStatus } from '@/ipc/bindings/GameSyncStatus';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import { checkpointDevice, pendingRemoteVersion, syncChip } from './game-view';

const device = (uuid: string, name: string, kind: DeviceMetadata['kind']): DeviceMetadata => ({
  device_uuid: uuid, name, platform: 'linux', first_seen: '', last_seen: '', kind,
});
const here = device('pc', 'Desktop', 'desktop');
const report = (extra: Partial<SyncReport> = {}): SyncReport => ({
  synced_at: '', games: [], remote_games: [], devices: [device('deck', 'Steam Deck', 'handheld')], remote_versions: {}, problems: [], ...extra,
});
const status = (relation: GameSyncStatus['relation'], extra: Partial<GameSyncStatus> = {}): GameSyncStatus => ({
  game_uuid: 'g', relation, remote_device: 'Steam Deck', remote_version_uuid: 'r', remote_version_id: 9, auto_applied: false, ...extra,
});

describe('checkpointDevice', () => {
  it('is this device for its own checkpoints and old ones without an origin', () => {
    expect(checkpointDevice(makeVersion({ id: 1, origin_device: 'pc' }), here, report())).toEqual({ here: true, name: null, kind: 'desktop' });
    expect(checkpointDevice(makeVersion({ id: 1 }), here, undefined).here).toBe(true);
  });

  it('names another device with its kind', () => {
    expect(checkpointDevice(makeVersion({ id: 1, origin_device: 'deck' }), here, report())).toEqual({ here: false, name: 'Steam Deck', kind: 'handheld' });
  });

  it('falls back to the synced-from name, then to no name', () => {
    const v = makeVersion({ id: 1 });
    expect(checkpointDevice(v, here, report({ remote_versions: { [v.uuid]: 'Laptop' } }))).toEqual({ here: false, name: 'Laptop', kind: 'desktop' });
    expect(checkpointDevice(makeVersion({ id: 2, origin_device: 'gone' }), here, undefined)).toEqual({ here: false, name: null, kind: 'desktop' });
  });

  it('assumes this device while it is loading', () => {
    expect(checkpointDevice(makeVersion({ id: 1, origin_device: 'pc' }), undefined, undefined).here).toBe(true);
  });
});

describe('syncChip', () => {
  const base = { on: true, running: false, reported: true };

  it('is off without a cloud folder', () => {
    expect(syncChip({ ...base, on: false, status: status('in_sync') })).toEqual({ kind: 'off' });
  });

  it('shows news from another device before anything else', () => {
    expect(syncChip({ ...base, running: true, status: status('fast_forward') })).toEqual({ kind: 'new-from', device: 'Steam Deck' });
    expect(syncChip({ ...base, status: status('diverged') })).toEqual({ kind: 'played-too', device: 'Steam Deck' });
  });

  it('shows a running sync, then where the game stands', () => {
    expect(syncChip({ ...base, running: true, status: status('in_sync') })).toEqual({ kind: 'sending' });
    expect(syncChip({ ...base, status: status('in_sync') })).toEqual({ kind: 'in-cloud' });
    expect(syncChip({ ...base, status: status('local_ahead') })).toEqual({ kind: 'not-in-cloud' });
    expect(syncChip({ ...base, status: undefined })).toEqual({ kind: 'not-in-cloud' });
  });
});

describe('pendingRemoteVersion', () => {
  it('is the fast-forward version unless it was cleaned up', () => {
    expect(pendingRemoteVersion(status('fast_forward'), new Set())).toBe(9);
    expect(pendingRemoteVersion(status('fast_forward'), new Set(['r']))).toBeNull();
    expect(pendingRemoteVersion(status('diverged'), new Set())).toBeNull();
    expect(pendingRemoteVersion(undefined, new Set())).toBeNull();
  });
});

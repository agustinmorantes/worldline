// Pure logic behind the game header and the checkpoint panel.
import type { DeviceKind } from '@/ipc/bindings/DeviceKind';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { GameSyncStatus } from '@/ipc/bindings/GameSyncStatus';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import type { Version } from '@/ipc/bindings/Version';

/** Where a checkpoint was made. `name` is null when another device isn't known by name. */
export interface CheckpointDevice {
  here: boolean;
  name: string | null;
  kind: DeviceKind;
}

export function checkpointDevice(version: Version, here: DeviceMetadata | undefined, report: SyncReport | undefined): CheckpointDevice {
  const synced = report?.remote_versions[version.uuid];
  const elsewhere = synced !== undefined
    || (version.origin_device !== null && here !== undefined && version.origin_device !== here.device_uuid);
  if (!elsewhere) return { here: true, name: null, kind: here?.kind ?? 'desktop' };
  const known = report?.devices.find((d) => d.device_uuid === version.origin_device);
  return { here: false, name: known?.name ?? synced ?? null, kind: known?.kind ?? 'desktop' };
}

export type SyncChip =
  | { kind: 'off' }
  | { kind: 'sending' }
  | { kind: 'in-cloud' }
  | { kind: 'not-in-cloud' }
  | { kind: 'new-from'; device: string }
  | { kind: 'played-too'; device: string };

export function syncChip({ on, running, reported, status }: {
  on: boolean;
  /** A sync is running right now. */
  running: boolean;
  /** A sync report has arrived. */
  reported: boolean;
  /** The game's entry in it. */
  status: GameSyncStatus | undefined;
}): SyncChip {
  if (!on) return { kind: 'off' };
  const device = status?.remote_device ?? null;
  if (status?.relation === 'fast_forward' && device !== null) return { kind: 'new-from', device };
  if (status?.relation === 'diverged' && device !== null) return { kind: 'played-too', device };
  if (running) return { kind: 'sending' };
  if (!reported) return { kind: 'in-cloud' };
  if (status === undefined || status.relation === 'local_only' || status.relation === 'local_ahead') return { kind: 'not-in-cloud' };
  return { kind: 'in-cloud' };
}

/** The other device's progress waiting to be applied here, unless it was cleaned up. */
export function pendingRemoteVersion(status: GameSyncStatus | undefined, retired: ReadonlySet<string>): number | null {
  if (status?.relation !== 'fast_forward' || status.remote_version_id === null) return null;
  if (status.remote_version_uuid !== null && retired.has(status.remote_version_uuid)) return null;
  return status.remote_version_id;
}

import { invoke } from '@tauri-apps/api/core';
import { Game, FullGame } from '@/models/game';
import {
  DeviceMetadata,
  DeviceMetadataSchema,
  SyncReport,
  SyncReportSchema,
  SyncSettings,
  SyncSettingsSchema,
} from '@/models/sync';

export async function getDevice(): Promise<DeviceMetadata> {
  return DeviceMetadataSchema.parse(await invoke('get_device'));
}

export async function updateDeviceName(name: string): Promise<DeviceMetadata> {
  return DeviceMetadataSchema.parse(await invoke('update_device', { name, kind: null }));
}

export async function getSyncSettings(): Promise<SyncSettings> {
  return SyncSettingsSchema.parse(await invoke('get_sync_settings'));
}

export async function updateSyncSettings(settings: SyncSettings): Promise<SyncSettings> {
  return SyncSettingsSchema.parse(await invoke('update_sync_settings', { settings }));
}

export async function refreshSync(): Promise<SyncReport> {
  return SyncReportSchema.parse(await invoke('refresh_sync'));
}

export async function adoptRemoteGame(gameUuid: string): Promise<FullGame> {
  const game = Game.fromData(await invoke('adopt_remote_game', { gameUuid }));
  if (game?.id === undefined) throw new Error('Invalid adopted game');
  return game as FullGame;
}

export async function applyRemoteVersion(versionId: number): Promise<void> {
  await invoke('apply_remote_version', { versionId });
}

export async function removeSyncDevice(deviceUuid: string): Promise<void> {
  await invoke('remove_sync_device', { deviceUuid });
}

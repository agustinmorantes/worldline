import { z } from 'zod';

export const DeviceMetadataSchema = z.object({
  device_uuid: z.uuid(),
  name: z.string(),
  platform: z.string(),
  first_seen: z.string(),
  last_seen: z.string(),
});

export const SyncSettingsSchema = z.object({
  remote_path: z.string().nullable().optional(),
});

export const SyncReportSchema = z.object({
  synced_at: z.string(),
  games: z.array(z.object({
    game_uuid: z.uuid(),
    relation: z.enum(['in_sync', 'fast_forward', 'local_ahead', 'diverged', 'remote_only', 'local_only']),
    remote_device: z.string().nullable().optional(),
    remote_version_uuid: z.uuid().nullable().optional(),
    remote_version_id: z.number().nullable().optional(),
    auto_applied: z.boolean(),
  })),
  remote_games: z.array(z.object({
    uuid: z.uuid(),
    title: z.string(),
    installed: z.boolean(),
    adopted: z.boolean(),
  })),
  devices: z.array(DeviceMetadataSchema),
  remote_versions: z.record(z.string(), z.string()),
});

export type DeviceMetadata = z.infer<typeof DeviceMetadataSchema>;
export type SyncSettings = z.infer<typeof SyncSettingsSchema>;
export type SyncReport = z.infer<typeof SyncReportSchema>;

import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';

const GcReportSchema = z.object({
  chunks_deleted: z.number(),
  bytes_deleted: z.number(),
  chunks_condemned: z.number(),
});

const PruningPreviewSchema = z.object({
  retire_versions: z.array(z.uuid()),
  live_bytes_before: z.number(),
  live_bytes_after: z.number(),
  budget_unreachable: z.boolean(),
  local_gc: GcReportSchema,
});

const PruningRunReportSchema = z.object({
  retired_versions: z.array(z.uuid()),
  budget_unreachable: z.boolean(),
  local_gc: GcReportSchema,
  remote_gc: GcReportSchema.nullable(),
});

export const RetentionSettingsSchema = z.object({
  automatic: z.boolean(),
  keep_all_hours: z.number(),
  hourly_until_hours: z.number(),
  daily_until_days: z.number(),
  weekly_until_days: z.number(),
  branch_expiry_days: z.number().nullable(),
  max_bytes: z.number().nullable(),
  updated_at: z.string(),
});

export type PruningPreview = z.infer<typeof PruningPreviewSchema>;
export type PruningRunReport = z.infer<typeof PruningRunReportSchema>;
export type RetentionSettings = z.infer<typeof RetentionSettingsSchema>;
export type RetentionTiers = Pick<
  RetentionSettings,
  'keep_all_hours' | 'hourly_until_hours' | 'daily_until_days' | 'weekly_until_days'
>;
export type RetentionPreset = 'dense' | 'balanced' | 'sparse';

/** Tier presets; any other combination is shown as "Custom". */
export const RETENTION_PRESETS: Record<RetentionPreset, RetentionTiers> = {
  dense: { keep_all_hours: 6, hourly_until_hours: 72, daily_until_days: 30, weekly_until_days: 180 },
  balanced: { keep_all_hours: 1, hourly_until_hours: 24, daily_until_days: 7, weekly_until_days: 90 },
  sparse: { keep_all_hours: 1, hourly_until_hours: 12, daily_until_days: 3, weekly_until_days: 30 },
};

export function presetOf(tiers: RetentionTiers): RetentionPreset | 'custom' {
  const match = (Object.keys(RETENTION_PRESETS) as RetentionPreset[]).find((name) => {
    const preset = RETENTION_PRESETS[name];
    return preset.keep_all_hours === tiers.keep_all_hours
      && preset.hourly_until_hours === tiers.hourly_until_hours
      && preset.daily_until_days === tiers.daily_until_days
      && preset.weekly_until_days === tiers.weekly_until_days;
  });
  return match ?? 'custom';
}

export async function getPruningSettings(): Promise<RetentionSettings> {
  return RetentionSettingsSchema.parse(await invoke('get_pruning_settings'));
}

export async function updatePruningSettings(settings: RetentionSettings): Promise<RetentionSettings> {
  return RetentionSettingsSchema.parse(await invoke('update_pruning_settings', { settings }));
}

export async function previewPruning(): Promise<PruningPreview> {
  return PruningPreviewSchema.parse(await invoke('preview_pruning'));
}

export async function runPruning(): Promise<PruningRunReport> {
  return PruningRunReportSchema.parse(await invoke('run_pruning'));
}

export async function getRetiredVersions(): Promise<string[]> {
  return z.array(z.uuid()).parse(await invoke('get_retired_versions'));
}

import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';

const BlobGcSchema = z.object({
  protected_blobs: z.number(),
  reclaimable_blobs: z.number(),
  reclaimable_bytes: z.number(),
});

const PruningPreviewSchema = z.object({
  retire_versions: z.array(z.uuid()),
  local_gc: BlobGcSchema,
});

const PruningRunReportSchema = z.object({
  retired_versions: z.array(z.uuid()),
  local_gc: BlobGcSchema,
  remote_gc: BlobGcSchema.nullable(),
});

const PruningSettingsSchema = z.object({ automatic: z.boolean() });

export type PruningPreview = z.infer<typeof PruningPreviewSchema>;
export type PruningRunReport = z.infer<typeof PruningRunReportSchema>;
export type PruningSettings = z.infer<typeof PruningSettingsSchema>;

export async function getPruningSettings(): Promise<PruningSettings> {
  return PruningSettingsSchema.parse(await invoke('get_pruning_settings'));
}

export async function updatePruningSettings(settings: PruningSettings): Promise<PruningSettings> {
  return PruningSettingsSchema.parse(await invoke('update_pruning_settings', { settings }));
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

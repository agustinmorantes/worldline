import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';

const TrackedDirectorySchema = z.object({
  uuid: z.uuid(),
  label: z.string().nullable(),
  source_path_hint: z.string(),
});

const HistoryBundleSummarySchema = z.object({
  game_uuid: z.uuid(),
  title: z.string(),
  slot_count: z.number(),
  version_count: z.number(),
  tracked_dir_count: z.number(),
  blob_count: z.number(),
  retired_version_count: z.number(),
  active_tracked_dirs: z.array(TrackedDirectorySchema),
});

const HistoryTransferReportSchema = HistoryBundleSummarySchema.omit({ active_tracked_dirs: true });

export type HistoryBundleSummary = z.infer<typeof HistoryBundleSummarySchema>;
export type HistoryTransferReport = z.infer<typeof HistoryTransferReportSchema>;

export async function inspectHistoryBundle(bundlePath: string): Promise<HistoryBundleSummary> {
  return HistoryBundleSummarySchema.parse(await invoke('inspect_history_bundle', { bundlePath }));
}

export async function exportGameHistory(gameId: number, destination: string): Promise<HistoryTransferReport> {
  return HistoryTransferReportSchema.parse(await invoke('export_game_history', { gameId, destination }));
}

export async function importGameHistory(
  bundlePath: string,
  mappings: Record<string, string>,
): Promise<HistoryTransferReport> {
  return HistoryTransferReportSchema.parse(await invoke('import_game_history', { bundlePath, mappings }));
}

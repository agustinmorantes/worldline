import { type QueryKey, type UseQueryResult, keepPreviousData, useQuery } from '@tanstack/react-query';
import { getDevice, getMonitoringSettings, getPruningSettings, previewPruning } from '@/ipc/commands';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';
import type { PruningPreview } from '@/ipc/bindings/PruningPreview';
import type { RetentionSettings } from '@/ipc/bindings/RetentionSettings';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';

export const useMonitoringSettings = (): UseQueryResult<MonitoringSettings> =>
  useQuery({ queryKey: ['monitoring-settings'], queryFn: getMonitoringSettings });

export const useRetention = (): UseQueryResult<RetentionSettings> =>
  useQuery({ queryKey: ['pruning-settings'], queryFn: getPruningSettings });

/** What a cleanup with `settings` would do; the last answer stays up while the next loads. */
export const usePruningPreview = (settings: RetentionSettings | undefined): UseQueryResult<PruningPreview> =>
  useQuery({
    queryKey: ['pruning-preview', settings],
    queryFn: () => previewPruning(settings),
    enabled: settings !== undefined,
    placeholderData: keepPreviousData,
  });

export const useDevice = (): UseQueryResult<DeviceMetadata> =>
  useQuery({ queryKey: ['device'], queryFn: getDevice });

let saves = Promise.resolve();

/**
 * Shows `next` at once and saves it in the background. Saves run one after another, so the
 * last change wins; a failure is reported and the saved value reloaded.
 */
export function saveNow<T>(key: QueryKey, next: T, save: (value: T) => Promise<unknown>, failed: string): void {
  queryClient.setQueryData(key, next);
  saves = saves.then(() => save(next)).then(
    () => undefined,
    (err: unknown) => {
      reportError(failed, err);
      void queryClient.invalidateQueries({ queryKey: key });
    },
  );
}

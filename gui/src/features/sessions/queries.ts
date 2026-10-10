import { type UseQueryResult, useQuery } from '@tanstack/react-query';
import { getDevice, getMonitoringSettings, getScreenshotPath, getUnrecordedChanges } from '@/ipc/commands';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';
import { utcDate } from '@/ipc/time';

export const useMonitoringSettings = (): UseQueryResult<MonitoringSettings> =>
  useQuery({ queryKey: ['monitoring-settings'], queryFn: getMonitoringSettings });

/** This device, as other devices see it. */
export const useDevice = (): UseQueryResult<DeviceMetadata> =>
  useQuery({ queryKey: ['device'], queryFn: getDevice });

/** The checkpoint's screenshot file; `screenshot-attached` refreshes it when one arrives. */
export const useScreenshot = (versionId: number): UseQueryResult<string | null> =>
  useQuery({
    queryKey: ['screenshot', versionId],
    queryFn: () => getScreenshotPath(versionId),
    meta: { errorMessage: (t) => t.checkpoint.screenshotFailed },
  });

/**
 * The screenshot in a dot's hover preview. Its own entry, so a failure stays quiet (hovering
 * along the timeline must not toast once per dot) without quieting the panel's.
 */
export const useScreenshotPreview = (versionId: number): UseQueryResult<string | null> =>
  useQuery({
    queryKey: ['screenshot', versionId, 'preview'],
    queryFn: () => getScreenshotPath(versionId),
    meta: { quiet: true },
    retry: false,
  });

/**
 * When the watcher records the game’s saves that changed since its current checkpoint; `null`
 * while they match it. Read once; `unrecorded-changes` keeps it current.
 */
export function useUnrecorded(gameId: number): { dueAt: Date } | null {
  const { data: dueAt } = useQuery({
    queryKey: ['unrecorded-changes'],
    queryFn: getUnrecordedChanges,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    select: (entries) => entries.find(([id]) => id === gameId)?.[1],
  });
  return dueAt === undefined ? null : { dueAt: utcDate(dueAt) };
}

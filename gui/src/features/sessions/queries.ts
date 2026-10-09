import { type UseQueryResult, useQuery } from '@tanstack/react-query';
import { getDevice, getMonitoringSettings, getScreenshotPath } from '@/ipc/commands';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';

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

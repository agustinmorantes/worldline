import { type UseQueryResult, useQuery } from '@tanstack/react-query';
import { getDevice, getMonitoringSettings } from '@/ipc/commands';
import type { DeviceMetadata } from '@/ipc/bindings/DeviceMetadata';
import type { MonitoringSettings } from '@/ipc/bindings/MonitoringSettings';

export const useMonitoringSettings = (): UseQueryResult<MonitoringSettings> =>
  useQuery({ queryKey: ['monitoring-settings'], queryFn: getMonitoringSettings });

/** This device, as other devices see it. */
export const useDevice = (): UseQueryResult<DeviceMetadata> =>
  useQuery({ queryKey: ['device'], queryFn: getDevice });

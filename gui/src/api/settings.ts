import { invoke } from '@tauri-apps/api/core';
import {
  MonitoringSettings,
  MonitoringSettingsSchema,
} from '@/models/monitoring-settings';

export async function getMonitoringSettings(): Promise<MonitoringSettings> {
  const response = await invoke('get_monitoring_settings');
  return MonitoringSettingsSchema.parse(response);
}

export async function updateMonitoringSettings(
  monitoringSettings: MonitoringSettings,
): Promise<MonitoringSettings> {
  const response = await invoke('update_monitoring_settings', {
    monitoringSettings,
  });
  return MonitoringSettingsSchema.parse(response);
}

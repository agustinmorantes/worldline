import { invoke } from '@tauri-apps/api/core';
import {
  MonitoringSettings,
  MonitoringSettingsSchema,
} from '@/models/monitoring-settings';

export async function getMonitoringSettings(): Promise<MonitoringSettings> {
  const response = await invoke('get_monitoring_settings');
  return MonitoringSettingsSchema.parse(response);
}

/** Set once when pre-release data was moved aside on startup. */
export async function getStartupNotice(): Promise<string | null> {
  const notice = await invoke('get_startup_notice');
  return typeof notice === 'string' ? notice : null;
}

export async function updateMonitoringSettings(
  monitoringSettings: MonitoringSettings,
): Promise<MonitoringSettings> {
  const response = await invoke('update_monitoring_settings', {
    monitoringSettings,
  });
  return MonitoringSettingsSchema.parse(response);
}

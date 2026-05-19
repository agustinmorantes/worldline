import { z } from 'zod';

export const MonitoringSettingsSchema = z.object({
  version_cooldown_minutes: z.number(),
  snapshot_debounce_seconds: z.number(),
});

export type MonitoringSettings = z.infer<typeof MonitoringSettingsSchema>;

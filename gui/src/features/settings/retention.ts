import type { RetentionSettings } from '@/ipc/bindings/RetentionSettings';

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

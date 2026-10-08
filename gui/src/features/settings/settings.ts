// Pure logic behind the Settings screen: stepper values, retention tiers and their ruler.
import type { PruningPreview } from '@/ipc/bindings/PruningPreview';
import type { RetentionSettings } from '@/ipc/bindings/RetentionSettings';
import type { Locale } from '@/shared/i18n';

export const WAIT_SECONDS = [1, 2, 3, 5, 10, 20, 30];
export const COOLDOWN_MINUTES = [1, 2, 5, 10, 15, 30, 60];

export type TierKey = 'keep_all_hours' | 'hourly_until_hours' | 'daily_until_days' | 'weekly_until_days';
export type Tiers = Pick<RetentionSettings, TierKey>;

/** Each tier's stepper values, in the unit core stores (hours or days). */
export const TIER_STEPS: Record<TierKey, readonly number[]> = {
  keep_all_hours: [1, 3, 6, 12, 24, 48, 72],
  hourly_until_hours: [24, 48, 72, 120, 168, 336],
  daily_until_days: [7, 14, 30, 60, 90],
  weekly_until_days: [30, 90, 180, 365, 730],
};

/** "Forget unnamed timelines after" in days; past the last step it is Off (`null`). */
export const FORGET_DAYS = [7, 14, 30, 60, 90];

/**
 * The next step from `value` in direction `dir`, or `value` itself at either end. A value
 * between steps (set from the CLI) moves to the neighbouring step.
 */
export function step(steps: readonly number[], value: number, dir: 1 | -1): number {
  const next = dir > 0 ? steps.find((s) => s > value) : [...steps].reverse().find((s) => s < value);
  return next ?? value;
}

/**
 * Steps one tier, then moves the others so they never overlap, which core rejects:
 * keep-all ≤ hourly ≤ daily ≤ weekly.
 */
export function stepTier(tiers: Tiers, key: TierKey, dir: 1 | -1): Tiers {
  const t = { ...tiers, [key]: step(TIER_STEPS[key], tiers[key], dir) };
  if (dir > 0) {
    t.hourly_until_hours = Math.max(t.hourly_until_hours, t.keep_all_hours);
    t.daily_until_days = Math.max(t.daily_until_days, Math.ceil(t.hourly_until_hours / 24));
    t.weekly_until_days = Math.max(t.weekly_until_days, t.daily_until_days);
  } else {
    t.daily_until_days = Math.min(t.daily_until_days, t.weekly_until_days);
    t.hourly_until_hours = Math.min(t.hourly_until_hours, t.daily_until_days * 24);
    t.keep_all_hours = Math.min(t.keep_all_hours, t.hourly_until_hours);
  }
  return t;
}

export function stepForget(days: number | null, dir: 1 | -1): number | null {
  if (days === null) return dir > 0 ? null : FORGET_DAYS[FORGET_DAYS.length - 1];
  const next = step(FORGET_DAYS, days, dir);
  return dir > 0 && next === days ? null : next;
}

export type RulerTier = 'all' | 'hourly' | 'daily' | 'weekly' | 'monthly';

/** Positions are fractions of the ruler's width; now is at the right edge. */
export interface RulerSegment { tier: RulerTier; left: number; width: number }
/** `n` counts the dots in a tier from now, so a dot keeps its identity as the tier moves. */
export interface RulerDot { tier: RulerTier; n: number; left: number }

/** The ruler's left edge, in hours ago: a bit past the longest weekly step. */
const HORIZON_HOURS = 730 * 24 * 1.25;
/** Space between dots, as a fraction of the width: denser near now. */
const DOT_GAP: Record<RulerTier, number> = { all: 0.011, hourly: 0.019, daily: 0.03, weekly: 0.048, monthly: 0.075 };

/** Where an age lands on the log-scaled ruler. */
const position = (hours: number): number => Math.max(0, 1 - Math.log1p(hours) / Math.log1p(HORIZON_HOURS));

/** The retention ruler: one segment per tier, oldest first, with dots thinning out with age. */
export function ruler(tiers: Tiers): { segments: RulerSegment[]; dots: RulerDot[] } {
  const ends: [RulerTier, number, number][] = [
    ['monthly', HORIZON_HOURS, tiers.weekly_until_days * 24],
    ['weekly', tiers.weekly_until_days * 24, tiers.daily_until_days * 24],
    ['daily', tiers.daily_until_days * 24, tiers.hourly_until_hours],
    ['hourly', tiers.hourly_until_hours, tiers.keep_all_hours],
    ['all', tiers.keep_all_hours, 0],
  ];
  const segments: RulerSegment[] = [];
  const dots: RulerDot[] = [];
  for (const [tier, from, to] of ends) {
    const left = position(from);
    const right = position(to);
    segments.push({ tier, left, width: right - left });
    for (let x = right - DOT_GAP.all / 2, n = 0; x >= left; x -= DOT_GAP[tier], n++) dots.push({ tier, n, left: x });
  }
  return { segments, dots };
}

/** What a cleanup with these settings would free: retired checkpoints plus files already unused. */
export function freedBytes(preview: PruningPreview): number {
  return Math.max(0, preview.live_bytes_before - preview.live_bytes_after) + preview.local_gc.bytes_deleted;
}

const BYTE_UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte'] as const;

/** "1.2 GB", "340 MB" (decimal units). */
export function formatBytes(bytes: number, locale: Locale): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: BYTE_UNITS[unit],
    unitDisplay: 'short',
    maximumFractionDigits: unit > 0 && value < 10 ? 1 : 0,
  }).format(value);
}

const OS_NAMES: Record<string, string> = { windows: 'Windows', linux: 'Linux', macos: 'macOS' };

/** The OS name for a device's `platform` (Rust's `std::env::consts::OS`). */
export function osName(platform: string): string {
  return OS_NAMES[platform] ?? platform;
}

import { describe, expect, it } from 'vitest';
import { formatBytes, freedBytes, osName, ruler, step, stepForget, stepTier, type Tiers } from './settings';

const DEFAULTS: Tiers = { keep_all_hours: 1, hourly_until_hours: 24, daily_until_days: 7, weekly_until_days: 90 };

describe('step', () => {
  it('moves to the neighbouring step and stops at the ends', () => {
    expect(step([1, 5, 10], 5, 1)).toBe(10);
    expect(step([1, 5, 10], 5, -1)).toBe(1);
    expect(step([1, 5, 10], 10, 1)).toBe(10);
    expect(step([1, 5, 10], 1, -1)).toBe(1);
  });

  it('snaps a value between steps to the next one', () => {
    expect(step([1, 5, 10], 7, 1)).toBe(10);
    expect(step([1, 5, 10], 7, -1)).toBe(5);
  });
});

describe('stepTier', () => {
  it('steps one tier', () => {
    expect(stepTier(DEFAULTS, 'daily_until_days', 1)).toEqual({ ...DEFAULTS, daily_until_days: 14 });
  });

  it('pushes the later tiers up so they never overlap', () => {
    const tiers = { ...DEFAULTS, keep_all_hours: 48, hourly_until_hours: 48 };
    expect(stepTier(tiers, 'keep_all_hours', 1)).toEqual({ ...DEFAULTS, keep_all_hours: 72, hourly_until_hours: 72 });
    const long = { ...DEFAULTS, hourly_until_hours: 168, daily_until_days: 7 };
    expect(stepTier(long, 'hourly_until_hours', 1)).toEqual({ ...long, hourly_until_hours: 336, daily_until_days: 14, weekly_until_days: 90 });
  });

  it('pulls the earlier tiers down so they never overlap', () => {
    const tiers = { ...DEFAULTS, daily_until_days: 60 };
    expect(stepTier(tiers, 'weekly_until_days', -1)).toEqual({ ...DEFAULTS, daily_until_days: 30, weekly_until_days: 30 });
    const long = { keep_all_hours: 72, hourly_until_hours: 336, daily_until_days: 14, weekly_until_days: 30 };
    expect(stepTier(long, 'daily_until_days', -1)).toEqual({ ...long, hourly_until_hours: 168, daily_until_days: 7 });
  });
});

describe('stepForget', () => {
  it('goes Off past the longest step and back', () => {
    expect(stepForget(30, 1)).toBe(60);
    expect(stepForget(90, 1)).toBeNull();
    expect(stepForget(null, 1)).toBeNull();
    expect(stepForget(null, -1)).toBe(90);
    expect(stepForget(7, -1)).toBe(7);
  });
});

describe('ruler', () => {
  it('lays the tiers out oldest first, filling the width, with dots inside their tier', () => {
    const { segments, dots } = ruler(DEFAULTS);
    expect(segments.map((s) => s.tier)).toEqual(['monthly', 'weekly', 'daily', 'hourly', 'all']);
    expect(segments[0].left).toBe(0);
    const last = segments[segments.length - 1];
    expect(last.left + last.width).toBeCloseTo(1);
    for (const dot of dots) {
      const segment = segments.find((s) => s.tier === dot.tier);
      expect(dot.left).toBeGreaterThanOrEqual(segment?.left ?? Infinity);
      expect(dot.left).toBeLessThanOrEqual((segment?.left ?? 0) + (segment?.width ?? 0));
    }
  });

  it('widens a tier as it grows', () => {
    const width = (tiers: Tiers): number => ruler(tiers).segments.find((s) => s.tier === 'weekly')?.width ?? 0;
    expect(width({ ...DEFAULTS, weekly_until_days: 365 })).toBeGreaterThan(width(DEFAULTS));
  });
});

describe('formatting', () => {
  it('estimates the space a cleanup frees', () => {
    expect(freedBytes({
      retire_versions: [],
      live_bytes_before: 5_000,
      live_bytes_after: 3_000,
      budget_unreachable: false,
      local_gc: { chunks_deleted: 1, bytes_deleted: 500, chunks_condemned: 0 },
    })).toBe(2_500);
  });

  it('formats sizes in decimal units', () => {
    expect(formatBytes(1_234_000_000, 'en')).toBe('1.2 GB');
    expect(formatBytes(340_000_000, 'en')).toBe('340 MB');
    expect(formatBytes(1_234_000_000, 'es')).toBe('1,2 GB');
  });

  it('names the OS', () => {
    expect(osName('windows')).toBe('Windows');
    expect(osName('freebsd')).toBe('freebsd');
  });
});

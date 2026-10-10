import { LOCALES, type LanguagePref } from '@/shared/i18n/locale';

/** Bump when a field changes meaning; `parsePrefs` then migrates older files. */
export const PREFS_VERSION = 1;

export const TONES = ['slate', 'black', 'warm'] as const;
export type Tone = (typeof TONES)[number];
export const MOTION_PREFS = ['system', 'full', 'reduced'] as const;
export type MotionPref = (typeof MOTION_PREFS)[number];
export const CORNERS = ['br', 'bl', 'tr', 'tl'] as const;
export type Corner = (typeof CORNERS)[number];
/** A game's own popup corner, or no popups while it plays. */
export type GameCorner = Corner | 'off';

/** The in-game popup (ADR 0009); pushed to Rust on every change. */
export interface OverlayPrefs {
  enabled: boolean;
  sound: boolean;
  corner: Corner;
  /** By game `uuid`: screens differ per device, so this isn't game meta. */
  perGame: Record<string, GameCorner>;
  quiet: boolean;
  warnAhead: boolean;
  quickKeepKey: string;
  nameItKey: string;
}

/** GUI-only preferences. Game colors are game meta, not preferences. */
export interface Prefs {
  language: LanguagePref;
  tone: Tone;
  motion: MotionPref;
  scanView: 'grid' | 'list';
  onboardingSeen: boolean;
  /** Contextual hints already shown, by id. */
  hints: Record<string, boolean>;
  overlay: OverlayPrefs;
}

export const DEFAULT_PREFS: Prefs = {
  language: 'system',
  tone: 'slate',
  motion: 'system',
  scanView: 'grid',
  onboardingSeen: false,
  hints: {},
  overlay: {
    enabled: true,
    sound: true,
    corner: 'br',
    perGame: {},
    quiet: false,
    warnAhead: true,
    quickKeepKey: 'Ctrl+Shift+K',
    nameItKey: 'Ctrl+Shift+N',
  },
};

export type StoredPrefs = Prefs & { version: number };

function oneOf<T extends string>(options: readonly T[], value: unknown, fallback: T): T {
  return options.find((option) => option === value) ?? fallback;
}

function flag(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

function parseOverlay(raw: unknown): OverlayPrefs {
  const fallback = DEFAULT_PREFS.overlay;
  if (typeof raw !== 'object' || raw === null) return fallback;
  const stored = raw as Partial<Record<keyof OverlayPrefs, unknown>>;
  const games: readonly GameCorner[] = [...CORNERS, 'off'];
  const perGame = typeof stored.perGame === 'object' && stored.perGame !== null && !Array.isArray(stored.perGame)
    ? Object.fromEntries(Object.entries(stored.perGame).filter(([, corner]) => games.includes(corner as GameCorner))) as Record<string, GameCorner>
    : fallback.perGame;
  return {
    enabled: flag(stored.enabled, fallback.enabled),
    sound: flag(stored.sound, fallback.sound),
    corner: oneOf(CORNERS, stored.corner, fallback.corner),
    perGame,
    quiet: flag(stored.quiet, fallback.quiet),
    warnAhead: flag(stored.warnAhead, fallback.warnAhead),
    quickKeepKey: typeof stored.quickKeepKey === 'string' ? stored.quickKeepKey : fallback.quickKeepKey,
    nameItKey: typeof stored.nameItKey === 'string' ? stored.nameItKey : fallback.nameItKey,
  };
}

/** Reads a stored file field by field; anything missing or invalid takes its default. */
export function parsePrefs(raw: unknown): Prefs {
  if (typeof raw !== 'object' || raw === null) return DEFAULT_PREFS;
  const stored = raw as Partial<Record<keyof StoredPrefs, unknown>>;
  const hints = typeof stored.hints === 'object' && stored.hints !== null
    ? Object.fromEntries(Object.entries(stored.hints).filter(([, seen]) => typeof seen === 'boolean'))
    : DEFAULT_PREFS.hints;
  return {
    language: oneOf(['system', ...LOCALES], stored.language, DEFAULT_PREFS.language),
    tone: oneOf(TONES, stored.tone, DEFAULT_PREFS.tone),
    motion: oneOf(MOTION_PREFS, stored.motion, DEFAULT_PREFS.motion),
    scanView: oneOf(['grid', 'list'], stored.scanView, DEFAULT_PREFS.scanView),
    onboardingSeen: typeof stored.onboardingSeen === 'boolean' ? stored.onboardingSeen : DEFAULT_PREFS.onboardingSeen,
    hints: hints,
    overlay: parseOverlay(stored.overlay),
  };
}

export function serializePrefs(prefs: Prefs): StoredPrefs {
  return { version: PREFS_VERSION, ...prefs };
}

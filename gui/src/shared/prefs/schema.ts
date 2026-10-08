import { LOCALES, type LanguagePref } from '@/shared/i18n/locale';

/** Bump when a field changes meaning; `parsePrefs` then migrates older files. */
export const PREFS_VERSION = 1;

export const TONES = ['slate', 'black', 'warm'] as const;
export type Tone = (typeof TONES)[number];
export const MOTION_PREFS = ['system', 'full', 'reduced'] as const;
export type MotionPref = (typeof MOTION_PREFS)[number];

/** GUI-only preferences. Game colors are game meta, not preferences. */
export interface Prefs {
  language: LanguagePref;
  tone: Tone;
  motion: MotionPref;
  onboardingSeen: boolean;
  /** Contextual hints already shown, by id. */
  hints: Record<string, boolean>;
}

export const DEFAULT_PREFS: Prefs = {
  language: 'system',
  tone: 'slate',
  motion: 'system',
  onboardingSeen: false,
  hints: {},
};

export type StoredPrefs = Prefs & { version: number };

function oneOf<T extends string>(options: readonly T[], value: unknown, fallback: T): T {
  return options.find((option) => option === value) ?? fallback;
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
    onboardingSeen: typeof stored.onboardingSeen === 'boolean' ? stored.onboardingSeen : DEFAULT_PREFS.onboardingSeen,
    hints: hints,
  };
}

export function serializePrefs(prefs: Prefs): StoredPrefs {
  return { version: PREFS_VERSION, ...prefs };
}

import { useReducedMotion, type MotionConfigProps, type Transition } from 'motion/react';
import { usePrefs } from '@/shared/prefs/prefs';
import type { MotionPref } from '@/shared/prefs/schema';

/** Default ease-out (`--ez`). */
export const ease = [0.2, 0.9, 0.25, 1] as const;
/** Playful overshoot for hover, pop and press (`--spring`). */
export const springEase = [0.34, 1.5, 0.5, 1] as const;
/** Physical spring for dots popping in and panes settling. */
export const spring: Transition = { type: 'spring', stiffness: 520, damping: 30, mass: 0.8 };

/** Durations in seconds; every state change sits in 200–600 ms. */
export const duration = { fast: 0.2, base: 0.32, slow: 0.6 } as const;

export function reducedMotionConfig(pref: MotionPref): NonNullable<MotionConfigProps['reducedMotion']> {
  switch (pref) {
    case 'system': return 'user';
    case 'full': return 'never';
    case 'reduced': return 'always';
  }
}

/** True when state changes should be instant: the Motion preference, or the OS setting under System. */
export function useMotionReduced(): boolean {
  const pref = usePrefs().motion;
  const system = useReducedMotion() === true;
  return pref === 'reduced' || (pref === 'system' && system);
}

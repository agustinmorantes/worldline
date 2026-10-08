// Pure logic behind the tour: the Lantern Knight demo game and the Worldline strip under it.
// `Tour.tsx` runs the timers that move it along.

/** 0 is the intro, 1–3 the lessons (checkpoints, rewind, timelines), 4 the finish. */
export type TourStep = 0 | 1 | 2 | 3 | 4;

export const LESSONS = [1, 2, 3] as const;
export type Lesson = (typeof LESSONS)[number];

export interface Demo {
  step: TourStep;
  /** Checkpoints the walk to the chest has made (0–3). */
  saved: number;
  /** The knight's x in the scene. */
  heroX: number;
  walking: boolean;
  /** The game's "Saving…" badge is up. */
  saving: boolean;
  /** A walk is under way; the action waits. */
  busy: boolean;
  /** The chest is open and the mimic is out. */
  mimic: boolean;
  /** The game autosaved after the mimic. */
  mimicSaved: boolean;
  rewinding: boolean;
  rewound: boolean;
  /** The knight walked past the chest to the castle gate. */
  walkedOn: boolean;
}

/** Where the knight stops on the way to the chest, and where those saves land on the strip. */
export const STOPS = [96, 196, 292] as const;
export const STRIP_X = [44, 144, 244] as const;
/** The chest's save, and the castle gate's after it, sit here on the strip. */
export const CHEST_X = 360;
export const GATE_HERO_X = 452;

/** A step as it starts: the lessons before it already played out. */
export function demoAt(step: TourStep): Demo {
  return {
    step,
    saved: step >= 2 ? 3 : 0,
    heroX: step >= 4 ? GATE_HERO_X : step >= 2 ? STOPS[2] : 24,
    walking: false,
    saving: false,
    busy: false,
    mimic: false,
    mimicSaved: step >= 3,
    rewinding: false,
    rewound: step >= 3,
    walkedOn: step >= 4,
  };
}

/** Where ← and → go: between the lessons, and from the last one on to the finish. */
export function arrowStep(step: TourStep, key: string): TourStep | null {
  if (key === 'ArrowRight' && step >= 1 && step <= 3) return (step + 1) as TourStep;
  if (key === 'ArrowLeft' && step >= 2 && step <= 3) return (step - 1) as TourStep;
  return null;
}

export type DemoAction =
  | { kind: 'play'; busy: boolean }
  | { kind: 'chest' }
  | { kind: 'hold'; ready: boolean }
  | { kind: 'walk'; busy: boolean }
  | { kind: 'next'; finish: boolean };

/** The one thing to do on a lesson: act in the game, then move on. */
export function demoAction(d: Demo): DemoAction {
  const next: DemoAction = { kind: 'next', finish: d.step === 3 };
  if (d.step === 1) return d.saved >= 3 ? next : { kind: 'play', busy: d.busy };
  if (d.step === 2) {
    if (d.rewound) return next;
    if (d.mimic || d.mimicSaved) return { kind: 'hold', ready: d.mimicSaved && !d.rewinding };
    return { kind: 'chest' };
  }
  return d.walkedOn ? next : { kind: 'walk', busy: d.busy };
}

/** Which of the lesson's lines describes where the demo is. */
export function demoLine(d: Demo): number {
  if (d.step === 1) return d.saved >= 3 ? 2 : d.busy ? 1 : 0;
  if (d.step === 2) return d.rewound ? 4 : d.rewinding ? 3 : d.mimicSaved ? 2 : d.mimic ? 1 : 0;
  return d.walkedOn ? 2 : d.busy ? 1 : 0;
}

/** The knight's gold: the mimic takes it, the rewind gives it back. */
export function gold(d: Demo): number {
  return d.mimic || (d.mimicSaved && !d.rewound) ? 0 : 120;
}

export type CheckpointName = 'camp' | 'forest' | 'chest' | 'mimic' | 'mimicRun' | 'gate';

export interface StripCheckpoint {
  name: CheckpointName;
  x: number;
  y: number;
  /** `bad`: the mimic save; `kept`: the same, hollow after the rewind; `lane`: on its own timeline. */
  look: 'main' | 'bad' | 'kept' | 'lane';
}

export interface Strip {
  checkpoints: StripCheckpoint[];
  /** "You are here" on the strip, once there is a checkpoint. */
  token: { x: number; bad: boolean } | null;
  /** The solid line runs up to here. */
  lineTo: number;
  /** After the rewind, the line ahead of you is dashed until you play on. */
  ahead: boolean;
  /** The mimic run has split into its own lane. */
  branch: boolean;
}

const MAIN_Y = 76;
const LANE_Y = 124;
const NAMES: CheckpointName[] = ['camp', 'forest', 'chest'];

export function stripOf(d: Demo): Strip {
  const checkpoints: StripCheckpoint[] = NAMES.slice(0, d.saved).map((name, i) => ({ name, x: STRIP_X[i] ?? 0, y: MAIN_Y, look: 'main' }));
  const back = d.rewinding || d.rewound;
  if (d.mimicSaved) {
    checkpoints.push(d.walkedOn
      ? { name: 'mimicRun', x: CHEST_X, y: LANE_Y, look: 'lane' }
      : { name: 'mimic', x: CHEST_X, y: MAIN_Y, look: back ? 'kept' : 'bad' });
  }
  if (d.walkedOn) checkpoints.push({ name: 'gate', x: CHEST_X, y: MAIN_Y, look: 'main' });

  const tokenX = d.walkedOn ? CHEST_X : back ? STRIP_X[2] : d.mimicSaved ? CHEST_X : d.saved > 0 ? (STRIP_X[d.saved - 1] ?? 0) : STRIP_X[0];
  return {
    checkpoints,
    token: d.saved > 0 ? { x: tokenX, bad: d.mimicSaved && !back && !d.walkedOn } : null,
    lineTo: d.saved > 0 ? tokenX : 20,
    ahead: back && !d.walkedOn,
    branch: d.walkedOn,
  };
}

import { describe, expect, it } from 'vitest';
import { arrowStep, demoAction, demoAt, demoLine, gold, stripOf, type Demo } from './tour';

const at = (step: Demo['step'], patch: Partial<Demo> = {}): Demo => ({ ...demoAt(step), ...patch });

describe('demoAt', () => {
  it('starts a lesson with the earlier ones played out', () => {
    expect(stripOf(at(1)).checkpoints).toEqual([]);
    expect(stripOf(at(2)).checkpoints.map((c) => c.name)).toEqual(['camp', 'forest', 'chest']);
    expect(stripOf(at(3)).checkpoints.map((c) => [c.name, c.look])).toEqual([['camp', 'main'], ['forest', 'main'], ['chest', 'main'], ['mimic', 'kept']]);
    expect(stripOf(at(4)).checkpoints.map((c) => [c.name, c.look])).toContainEqual(['mimicRun', 'lane']);
  });
});

describe('arrowStep', () => {
  it('moves between the lessons and on to the finish', () => {
    expect(arrowStep(1, 'ArrowRight')).toBe(2);
    expect(arrowStep(3, 'ArrowRight')).toBe(4);
    expect(arrowStep(3, 'ArrowLeft')).toBe(2);
  });

  it('leaves the intro and the finish to their buttons', () => {
    expect(arrowStep(0, 'ArrowRight')).toBeNull();
    expect(arrowStep(1, 'ArrowLeft')).toBeNull();
    expect(arrowStep(4, 'ArrowLeft')).toBeNull();
    expect(arrowStep(2, 'Enter')).toBeNull();
  });
});

describe('demoAction and demoLine', () => {
  it('plays, then moves on, in the checkpoints lesson', () => {
    expect(demoAction(at(1))).toEqual({ kind: 'play', busy: false });
    expect(demoAction(at(1, { busy: true, saved: 1 }))).toEqual({ kind: 'play', busy: true });
    expect(demoLine(at(1, { busy: true }))).toBe(1);
    expect(demoAction(at(1, { saved: 3 }))).toEqual({ kind: 'next', finish: false });
    expect(demoLine(at(1, { saved: 3 }))).toBe(2);
  });

  it('opens the chest, waits for the autosave, then holds to rewind', () => {
    expect(demoAction(at(2))).toEqual({ kind: 'chest' });
    expect(demoAction(at(2, { mimic: true }))).toEqual({ kind: 'hold', ready: false });
    expect(demoAction(at(2, { mimic: true, mimicSaved: true }))).toEqual({ kind: 'hold', ready: true });
    expect(demoAction(at(2, { mimicSaved: true, rewinding: true }))).toEqual({ kind: 'hold', ready: false });
    expect(demoLine(at(2, { mimicSaved: true, rewinding: true }))).toBe(3);
    expect(demoAction(at(2, { mimicSaved: true, rewound: true }))).toEqual({ kind: 'next', finish: false });
  });

  it('walks on, then finishes, in the timelines lesson', () => {
    expect(demoAction(at(3))).toEqual({ kind: 'walk', busy: false });
    expect(demoAction(at(3, { walkedOn: true }))).toEqual({ kind: 'next', finish: true });
  });
});

describe('gold', () => {
  it('is lost to the mimic and comes back with the rewind', () => {
    expect(gold(at(2))).toBe(120);
    expect(gold(at(2, { mimic: true }))).toBe(0);
    expect(gold(at(2, { mimicSaved: true }))).toBe(0);
    expect(gold(at(2, { mimicSaved: true, rewound: true }))).toBe(120);
  });
});

describe('stripOf', () => {
  it('has no line or token before the first save', () => {
    expect(stripOf(at(1))).toMatchObject({ token: null, lineTo: 20, ahead: false, branch: false });
  });

  it('marks the mimic save, then keeps it hollow behind you after the rewind', () => {
    const saved = stripOf(at(2, { mimicSaved: true }));
    expect(saved.token).toEqual({ x: 360, bad: true });
    expect(saved.checkpoints[saved.checkpoints.length - 1]?.look).toBe('bad');

    const back = stripOf(at(2, { mimicSaved: true, rewound: true }));
    expect(back.token).toEqual({ x: 244, bad: false });
    expect(back.checkpoints[back.checkpoints.length - 1]?.look).toBe('kept');
    expect(back.ahead).toBe(true);
  });

  it('splits the mimic run into its own lane once you play on', () => {
    const strip = stripOf(at(4));
    expect(strip.branch).toBe(true);
    expect(strip.ahead).toBe(false);
    expect(strip.token).toEqual({ x: 360, bad: false });
    expect(strip.checkpoints.find((c) => c.name === 'gate')).toMatchObject({ x: 360, y: 76 });
    expect(strip.checkpoints.find((c) => c.name === 'mimicRun')).toMatchObject({ x: 360, y: 124 });
  });
});

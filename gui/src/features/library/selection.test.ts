import { describe, expect, it } from 'vitest';
import { noSelection, select, selectionModifiers } from './selection';

// The list's order, unlike the ids'.
const order = [40, 20, 10, 30];
const ctrl = { toggle: true, range: false };
const shift = { toggle: false, range: true };
const ctrlShift = { toggle: true, range: true };

describe('selectionModifiers', () => {
  it('reads Ctrl or ⌘ as adding one, Shift as a run, and nothing as a plain click', () => {
    const keys = { ctrlKey: false, metaKey: false, shiftKey: false };
    expect(selectionModifiers(keys)).toBeNull();
    expect(selectionModifiers({ ...keys, ctrlKey: true })).toEqual(ctrl);
    expect(selectionModifiers({ ...keys, metaKey: true })).toEqual(ctrl);
    expect(selectionModifiers({ ...keys, shiftKey: true })).toEqual(shift);
    expect(selectionModifiers({ ...keys, ctrlKey: true, shiftKey: true })).toEqual(ctrlShift);
  });
});

describe('select', () => {
  it('adds and takes out one game with Ctrl, keeping the list’s order', () => {
    let sel = select(noSelection, 10, order, ctrl, null);
    sel = select(sel, 40, order, ctrl, null);
    expect(sel).toEqual({ ids: [40, 10], anchor: 40 });
    expect(select(sel, 10, order, ctrl, null)).toEqual({ ids: [40], anchor: 10 });
  });

  it('starts from the game on screen, as it is already highlighted', () => {
    expect(select(noSelection, 30, order, ctrl, 20)).toEqual({ ids: [20, 30], anchor: 30 });
    expect(select(noSelection, 30, order, shift, 20)).toEqual({ ids: [20, 10, 30], anchor: 20 });
    // Unless a search hides it.
    expect(select(noSelection, 30, [10, 30], ctrl, 20)).toEqual({ ids: [30], anchor: 30 });
  });

  it('takes the run in the list’s order from the last game picked with Ctrl', () => {
    const sel = select(noSelection, 40, order, ctrl, null);
    expect(select(sel, 10, order, shift, null)).toEqual({ ids: [40, 20, 10], anchor: 40 });
    // Upwards too, and a second Shift click moves the run's far end.
    const up = select(select(noSelection, 30, order, ctrl, null), 20, order, shift, null);
    expect(up).toEqual({ ids: [20, 10, 30], anchor: 30 });
    expect(select(up, 10, order, shift, null)).toEqual({ ids: [10, 30], anchor: 30 });
  });

  it('adds the run to what was picked with Ctrl+Shift', () => {
    let sel = select(noSelection, 40, order, ctrl, null);
    sel = select(sel, 10, order, ctrl, null);
    expect(select(sel, 30, order, ctrlShift, null)).toEqual({ ids: [40, 10, 30], anchor: 10 });
  });

  it('picks one game with Shift when there is nothing to run from', () => {
    expect(select(noSelection, 20, order, shift, null)).toEqual({ ids: [20], anchor: 20 });
  });

  it('drops games the list no longer shows', () => {
    const sel = { ids: [40, 20], anchor: 20 };
    expect(select(sel, 10, [20, 10], ctrl, null)).toEqual({ ids: [20, 10], anchor: 10 });
  });
});

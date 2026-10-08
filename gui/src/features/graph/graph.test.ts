import { describe, expect, it } from 'vitest';
import { makeVersion, naive } from '@/ipc/builders';
import type { Version } from '@/ipc/bindings/Version';
import { lineage } from '@/features/sessions/sessions';
import { graphNavOrder, LANE_GAP, LANE_X, layoutGraph } from './graph';

const MIN = 60_000;
const T0 = Date.UTC(2026, 9, 7, 18, 0);
const GAP = 40 * MIN;

function v(id: number, parent: number | null, minutes: number, extra: Partial<Version> = {}): Version {
  return makeVersion({ id, parent_id: parent, created_at: naive(new Date(T0 + minutes * MIN)), ...extra });
}

function line(first: number, parent: number | null, minutes: number[]): Version[] {
  return minutes.map((m, i) => v(first + i, i === 0 ? parent : first + i - 1, m));
}

function layout(versions: Version[], activeId: number): ReturnType<typeof layoutGraph> {
  return layoutGraph({ versions, mine: lineage(versions, activeId), gapMs: GAP });
}

const ys = (g: ReturnType<typeof layoutGraph>, ids: number[]): number[] => ids.map((id) => g.points.get(id)?.y ?? NaN);
const lanes = (g: ReturnType<typeof layoutGraph>, ids: number[]): number[] => ids.map((id) => g.points.get(id)?.lane ?? NaN);

describe('layoutGraph', () => {
  it('puts a history without branches on the spine, newest at the top', () => {
    const g = layout(line(1, null, [0, 10, 20, 100, 110]), 5);
    expect(lanes(g, [1, 2, 3, 4, 5])).toEqual([0, 0, 0, 0, 0]);
    const [a, b, c, d, e] = ys(g, [1, 2, 3, 4, 5]);
    expect(e).toBeLessThan(d);
    expect(d).toBeLessThan(c);
    expect(c).toBeLessThan(b);
    expect(b).toBeLessThan(a);
    // Two sessions: a wider gap between them, one capsule each, one edge.
    expect(c - d).toBeGreaterThan(d - e);
    expect(g.clusters.map((k) => k.versions.map((x) => x.id))).toEqual([[5, 4], [3, 2, 1]]);
    expect(g.capsules).toHaveLength(2);
    expect(g.edges).toHaveLength(1);
    expect(g.edges[0].d).toBe(`M ${LANE_X.toString()} ${a.toString()} L ${LANE_X.toString()} ${e.toString()}`);
    expect(g.labelX).toBe(LANE_X + 24);
  });

  it('gives a day header to each calendar day', () => {
    const g = layout(line(1, null, [0, 10, 24 * 60, 24 * 60 + 10]), 4);
    expect(g.days).toHaveLength(2);
    expect(g.days[0].y).toBeLessThan(g.days[1].y);
    expect(g.clusters).toHaveLength(2);
  });

  it('splits other timelines into lanes, nesting a timeline off a timeline', () => {
    // Your line 1→2→3→4. A fork off 2 (5→6), and a fork off 5 (7).
    const versions = [...line(1, null, [0, 10, 20, 30]), v(5, 2, 12), v(6, 5, 14), v(7, 5, 16)];
    const g = layout(versions, 4);
    expect(lanes(g, [1, 2, 3, 4])).toEqual([0, 0, 0, 0]);
    // Chains are read from the newest checkpoint: 7→5 is one, 6 nests off 5 in a lane of its own.
    const [l5, l6, l7] = lanes(g, [5, 6, 7]);
    expect(l7).toBe(l5);
    expect(l5).toBeGreaterThan(0);
    expect(l6).toBeGreaterThan(0);
    expect(l6).not.toBe(l5);
    expect(g.points.get(6)?.x).toBe(LANE_X + l6 * LANE_GAP);
    // The nested chain curves out of its parent's lane, not the spine.
    const nested = g.edges.find((e) => e.versions.some((x) => x.id === 6));
    expect(nested?.d.startsWith(`M ${(LANE_X + l5 * LANE_GAP).toString()} `)).toBe(true);
    expect(g.edges).toHaveLength(3);
    // Labels clear every lane.
    expect(g.labelX).toBe(LANE_X + 2 * LANE_GAP + 24);
  });

  it('lets timelines that do not overlap in time share a lane', () => {
    const versions = [...line(1, null, [0, 10, 200, 210]), v(5, 1, 5), v(6, 3, 205)];
    const g = layout(versions, 4);
    expect(lanes(g, [5, 6])).toEqual([1, 1]);
  });

  it('lines up every name in one column without overlaps', () => {
    const names = ['Meg defeated', 'Elysium', 'Styx', 'Hades', 'Credits'];
    const versions = names.map((tag, i) => v(i + 1, i === 0 ? null : i, i * 2, { tag }));
    const g = layout([...versions, v(6, 2, 3, { tag: 'Side trip' })], 5);
    expect(g.labels).toHaveLength(6);
    const ysOfLabels = g.labels.map((l) => l.y).sort((a, b) => a - b);
    for (let i = 1; i < ysOfLabels.length; i++) expect(ysOfLabels[i] - ysOfLabels[i - 1]).toBeGreaterThanOrEqual(20);
    const side = g.labels.find((l) => l.version.id === 6);
    expect(side?.lane).toBe(1);
    expect(side?.from).toBe(LANE_X + LANE_GAP + 9);
  });

  it('is empty without checkpoints', () => {
    const g = layoutGraph({ versions: [], mine: [], gapMs: GAP });
    expect(g.points.size).toBe(0);
    expect(g.edges).toEqual([]);
  });
});

describe('graphNavOrder', () => {
  it('reads top to bottom, skipping cleaned-up checkpoints', () => {
    const versions = [...line(1, null, [0, 10, 20]), v(4, 1, 20, { retired: true }), v(5, 1, 20)];
    const g = layout(versions, 3);
    const order = graphNavOrder(g, versions);
    expect(order).not.toContain(4);
    expect(order[order.length - 1]).toBe(1);
    expect(order).toHaveLength(4);
  });
});

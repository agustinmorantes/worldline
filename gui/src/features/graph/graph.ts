// Pure layout behind the Graph view: every checkpoint of every timeline on one time axis,
// newest at the top. Your line is the straight spine in lane 0; other timelines split off it
// into lanes of their own, sharing a lane when they don't overlap in time.
import type { Version } from '@/ipc/bindings/Version';
import { startOfDay, timeOf } from '@/features/sessions/sessions';

/** Where the spine sits, and the distance between lanes. */
export const LANE_X = 112;
export const LANE_GAP = 30;
const laneX = (lane: number): number => LANE_X + lane * LANE_GAP;
/** An SVG path from commands and coordinates. */
const path = (...parts: (string | number)[]): string => parts.join(' ');

export interface GraphPoint {
  x: number;
  y: number;
  lane: number;
}

/** Checkpoints close in time, read across every lane: one time label on the left. */
export interface GraphCluster {
  /** Top of the cluster's time label. */
  y: number;
  /** Newest first. */
  versions: Version[];
}

/** A lane's checkpoints within one cluster: drawn as a capsule. */
export interface GraphCapsule {
  lane: number;
  x: number;
  y: number;
  height: number;
  versions: Version[];
}

/** The spine, or another timeline's line from where it leaves its parent. */
export interface GraphEdge {
  lane: number;
  /** SVG path. */
  d: string;
  versions: Version[];
}

/** A named checkpoint's label, in one column right of every lane. */
export interface GraphLabel {
  version: Version;
  lane: number;
  y: number;
  /** Where its leader line starts: just right of the dot. */
  from: number;
}

export interface GraphLayout {
  points: Map<number, GraphPoint>;
  days: { y: number; day: number }[];
  clusters: GraphCluster[];
  capsules: GraphCapsule[];
  edges: GraphEdge[];
  labels: GraphLabel[];
  /** The left edge of the label column. */
  labelX: number;
  height: number;
}

const byTimeDesc = (a: Version, b: Version): number => timeOf(b) - timeOf(a) || b.id - a.id;
const tagged = (version: Version): boolean => version.tag !== null && version.tag !== '';

/**
 * Lay out every checkpoint of a playthrough.
 * @param mine Your current line, oldest first.
 */
export function layoutGraph({ versions, mine, gapMs }: { versions: Version[]; mine: Version[]; gapMs: number }): GraphLayout {
  const byId = new Map(versions.map((x) => [x.id, x]));
  const onLine = new Set(mine.map((x) => x.id));
  const all = [...versions].sort(byTimeDesc);

  // Rows: day headers, a wider gap between clusters, a little extra room around names.
  const days: GraphLayout['days'] = [];
  const clusters: GraphCluster[] = [];
  const ys = new Map<number, number>();
  const clusterOf = new Map<number, number>();
  let y = 10;
  let prev: Version | undefined;
  for (const version of all) {
    const day = startOfDay(new Date(timeOf(version)));
    if (prev === undefined || day !== days[days.length - 1].day) {
      if (prev !== undefined) y += 26;
      days.push({ y, day });
      y += 40;
      clusters.push({ y: 0, versions: [] });
    } else if (timeOf(prev) - timeOf(version) > gapMs) {
      y += 36;
      clusters.push({ y: 0, versions: [] });
    } else {
      y += 12 + (tagged(prev) ? 5 : 0) + (tagged(version) ? 5 : 0);
    }
    const cluster = clusters[clusters.length - 1];
    if (cluster.versions.length === 0) cluster.y = y - 10;
    cluster.versions.push(version);
    ys.set(version.id, y);
    clusterOf.set(version.id, clusters.length - 1);
    prev = version;
  }

  // Other timelines as chains, newest first, each ending where it meets your line or an
  // earlier chain (a timeline off a timeline).
  const chainOf = new Map<number, Chain>();
  const chains: Chain[] = [];
  for (const version of all) {
    if (onLine.has(version.id) || chainOf.has(version.id)) continue;
    const chain: Chain = { versions: [], parent: null, lane: 0, top: 0, bottom: 0 };
    let current: Version | undefined = version;
    while (current !== undefined && !onLine.has(current.id) && !chainOf.has(current.id)) {
      chainOf.set(current.id, chain);
      chain.versions.push(current);
      current = current.parent_id === null ? undefined : byId.get(current.parent_id);
    }
    chain.parent = current ?? null;
    chain.top = yOf(ys, chain.versions[0]);
    chain.bottom = yOf(ys, chain.parent ?? chain.versions[chain.versions.length - 1]);
    chains.push(chain);
  }

  // Lanes: the first one free over the chain's span, with some breathing room.
  const occupied: [number, number][][] = [[]];
  for (const chain of [...chains].sort((a, b) => a.top - b.top)) {
    let lane = 1;
    while (occupied[lane]?.some(([top, bottom]) => chain.top < bottom + 16 && top < chain.bottom + 16)) lane++;
    (occupied[lane] ??= []).push([chain.top, chain.bottom]);
    chain.lane = lane;
  }
  const laneOf = (version: Version): number => chainOf.get(version.id)?.lane ?? 0;

  const points = new Map<number, GraphPoint>();
  for (const version of all) {
    const lane = laneOf(version);
    points.set(version.id, { x: laneX(lane), y: yOf(ys, version), lane });
  }

  const edges: GraphEdge[] = [];
  if (mine.length > 0) {
    const x = laneX(0);
    edges.push({ lane: 0, d: path('M', x, yOf(ys, mine[0]), 'L', x, yOf(ys, mine[mine.length - 1])), versions: mine });
  }
  for (const chain of chains) {
    const x = laneX(chain.lane);
    const top = yOf(ys, chain.versions[0]);
    const oldest = yOf(ys, chain.versions[chain.versions.length - 1]);
    let d = path('M', x, oldest, 'L', x, top);
    if (chain.parent !== null) {
      // Curve out of the parent's lane, then straight up to the newest checkpoint.
      const py = yOf(ys, chain.parent);
      const px = laneX(laneOf(chain.parent));
      const h = Math.min(30, py - oldest);
      d = path('M', px, py, 'C', px, py - h * 0.6, x, py - h * 0.4, x, py - h, 'L', x, top);
    }
    edges.push({ lane: chain.lane, d, versions: chain.versions });
  }

  const groups = new Map<string, Version[]>();
  for (const version of all) {
    const key = `${laneOf(version).toString()}:${(clusterOf.get(version.id) ?? 0).toString()}`;
    groups.set(key, [...(groups.get(key) ?? []), version]);
  }
  const capsules = [...groups.values()].map((list): GraphCapsule => {
    const top = yOf(ys, list[0]);
    const bottom = yOf(ys, list[list.length - 1]);
    const lane = laneOf(list[0]);
    return { lane, x: laneX(lane) - 7, y: top - 7, height: bottom - top + 14, versions: list };
  });

  const labelX = laneX(occupied.length - 1) + 24;
  const labels = all.filter(tagged).map((version): GraphLabel => {
    const point = points.get(version.id) ?? { x: 0, y: 0, lane: 0 };
    return { version, lane: point.lane, y: point.y, from: point.x + 9 };
  });

  return { points, days, clusters, capsules, edges, labels, labelX, height: y + 60 };
}

interface Chain {
  /** Newest first. */
  versions: Version[];
  /** Where it leaves: a checkpoint on your line or on another chain. */
  parent: Version | null;
  lane: number;
  top: number;
  bottom: number;
}

function yOf(ys: Map<number, number>, version: Version): number {
  return ys.get(version.id) ?? 0;
}

/** Checkpoints in keyboard order: top to bottom, rightmost lane first on a tie. */
export function graphNavOrder(layout: GraphLayout, versions: Version[]): number[] {
  return versions
    .filter((v) => !v.retired && layout.points.has(v.id))
    .map((v) => ({ id: v.id, point: layout.points.get(v.id) ?? { x: 0, y: 0, lane: 0 } }))
    .sort((a, b) => a.point.y - b.point.y || b.point.x - a.point.x)
    .map((v) => v.id);
}

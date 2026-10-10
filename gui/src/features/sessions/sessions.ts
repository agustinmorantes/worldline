// Pure logic behind the Sessions view: play sessions derived from checkpoint timestamps,
// your current line, the other timelines folded into forks, and keyboard order.
import type { Version } from '@/ipc/bindings/Version';
import { utcDate } from '@/ipc/time';
import type { Locale } from '@/shared/i18n';

const MINUTE = 60_000;

export const timeOf = (version: Version): number => utcDate(version.created_at).getTime();

const byTime = (a: Version, b: Version): number => timeOf(a) - timeOf(b) || a.id - b.id;

/** A gap longer than this starts a new session. */
export function sessionGapMs(cooldownMinutes: number): number {
  return Math.max(40 * MINUTE, 2.5 * cooldownMinutes * MINUTE);
}

/** The active version and its ancestors, oldest first. */
export function lineage(versions: Version[], activeId: number | null): Version[] {
  const byId = new Map(versions.map((x) => [x.id, x]));
  const out: Version[] = [];
  let current = activeId === null ? undefined : byId.get(activeId);
  while (current !== undefined) {
    out.push(current);
    current = current.parent_id === null ? undefined : byId.get(current.parent_id);
  }
  return out.reverse();
}

/** Sessions, oldest first: split on a long gap or a change of device. */
export function splitSessions(list: Version[], gapMs: number): Version[][] {
  const sessions: Version[][] = [];
  let prev: Version | undefined;
  for (const version of [...list].sort(byTime)) {
    if (prev === undefined || timeOf(version) - timeOf(prev) > gapMs || version.origin_device !== prev.origin_device) {
      sessions.push([]);
    }
    sessions[sessions.length - 1].push(version);
    prev = version;
  }
  return sessions;
}

/** Approximate play time: each checkpoint's gap to its parent, when it is within a session. */
export function playedMs(list: Version[], all: Version[], gapMs: number): number {
  const byId = new Map(all.map((x) => [x.id, x]));
  let total = 0;
  for (const version of list) {
    const parent = version.parent_id === null ? undefined : byId.get(version.parent_id);
    if (parent === undefined) continue;
    const gap = timeOf(version) - timeOf(parent);
    if (gap > 0 && gap <= gapMs) total += gap;
  }
  return total;
}

/**
 * Play time across every timeline: the spans `playedMs` counts, each stretch of the clock once.
 * After a rewind, the new line's first span overlaps the one you left.
 */
export function totalPlayedMs(versions: Version[], gapMs: number): number {
  const byId = new Map(versions.map((x) => [x.id, x]));
  const spans = versions.flatMap((version): [number, number][] => {
    const parent = version.parent_id === null ? undefined : byId.get(version.parent_id);
    if (parent === undefined) return [];
    const from = timeOf(parent);
    const to = timeOf(version);
    return to > from && to - from <= gapMs ? [[from, to]] : [];
  });
  let total = 0;
  let end = -Infinity;
  for (const [from, to] of spans.sort((a, b) => a[0] - b[0])) {
    if (to <= end) continue;
    total += to - Math.max(from, end);
    end = to;
  }
  return total;
}

export function durationParts(ms: number): { hours: number; minutes: number } {
  const total = Math.round(ms / MINUTE);
  return { hours: Math.floor(total / 60), minutes: total % 60 };
}

/** A session's track length in px: longer sessions get longer tracks, within bounds. */
export function trackWidth(durationMs: number): number {
  return Math.round(Math.max(40, Math.min(380, 40 + (durationMs / (240 * MINUTE)) * 340)));
}

/** Where a checkpoint sits along its session's track, 0–1. */
export function trackPosition(version: Version, session: Version[]): number {
  if (session.length < 2) return 0.5;
  const first = timeOf(session[0]);
  return (timeOf(version) - first) / Math.max(1, timeOf(session[session.length - 1]) - first);
}

/** Local midnight of `date`, as a timestamp. */
export const startOfDay = (date: Date): number => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();

/** Calendar days from `date` to `now`, in local time. */
function daysAgo(date: Date, now: Date): number {
  return Math.round((startOfDay(now) - startOfDay(date)) / (24 * 60 * MINUTE));
}

/** Today, Yesterday, the weekday within the last week, then the date. */
export function dayLabel(date: Date, now: Date, locale: Locale, words: { today: string; yesterday: string }): string {
  const ago = daysAgo(date, now);
  if (ago <= 0) return words.today;
  if (ago === 1) return words.yesterday;
  if (ago < 7) return new Intl.DateTimeFormat(locale, { weekday: 'long' }).format(date);
  return new Intl.DateTimeFormat(locale, { weekday: 'short', month: 'short', day: 'numeric' }).format(date);
}

/** Another timeline, folded into a chip at its fork point. */
export interface Fork {
  /** The first checkpoint off your line; identifies the fork. */
  root: Version;
  /** Where it leaves your line; `null` for a history with no common start. */
  from: Version | null;
  /** Every checkpoint in it, oldest first. */
  line: Version[];
  /** Its sessions, oldest first. */
  sessions: Version[][];
  /** It holds the playthrough's newest checkpoint: where you left off before coming back. */
  latest: boolean;
}

export interface SessionRow {
  /** Oldest first. */
  versions: Version[];
  durationMs: number;
  /** Timelines leaving from this session. */
  forks: Fork[];
}

export interface DayGroup {
  /** Local midnight, as a timestamp. */
  day: number;
  /** Newest first. */
  rows: SessionRow[];
}

export interface SessionsLayout {
  /** Your current line, oldest first. */
  lineage: Version[];
  /** Newest first. */
  days: DayGroup[];
  /** Another device's progress ahead of you, oldest first, when it is waiting to be applied. */
  ghost: Version[] | null;
}

export function layoutSessions({ versions, activeId, gapMs, pendingId = null }: {
  versions: Version[];
  activeId: number | null;
  gapMs: number;
  /** The fast-forward version waiting to be applied. */
  pendingId?: number | null;
}): SessionsLayout {
  if (versions.length === 0) return { lineage: [], days: [], ghost: null };
  const byId = new Map(versions.map((x) => [x.id, x]));
  const newest = [...versions].sort(byTime)[versions.length - 1];
  const head = activeId !== null && byId.has(activeId) ? activeId : newest.id;
  const mine = lineage(versions, head);
  const onLine = new Set(mine.map((x) => x.id));

  const children = new Map<number | null, Version[]>();
  for (const version of versions) {
    const parent = version.parent_id !== null && byId.has(version.parent_id) ? version.parent_id : null;
    children.set(parent, [...(children.get(parent) ?? []), version]);
  }
  const subtree = (root: Version): Version[] => {
    const out: Version[] = [];
    const stack = [root];
    for (let next = stack.pop(); next !== undefined; next = stack.pop()) {
      out.push(next);
      stack.push(...(children.get(next.id) ?? []));
    }
    return out.sort(byTime);
  };

  let ghost: Version[] | null = null;
  const forksFrom = new Map<number | null, Fork[]>();
  for (const [parent, kids] of children) {
    if (parent !== null && !onLine.has(parent)) continue;
    for (const root of kids) {
      if (onLine.has(root.id)) continue;
      const line = subtree(root);
      if (pendingId !== null && line.some((x) => x.id === pendingId)) {
        ghost = lineage(versions, pendingId).filter((x) => !onLine.has(x.id));
        continue;
      }
      const fork = { root, from: parent === null ? null : (byId.get(parent) ?? null), line, sessions: splitSessions(line, gapMs), latest: line.includes(newest) };
      forksFrom.set(parent, [...(forksFrom.get(parent) ?? []), fork]);
    }
  }

  const sessions = splitSessions(mine, gapMs);
  const rows = sessions.map((list, i): SessionRow => ({
    versions: list,
    durationMs: timeOf(list[list.length - 1]) - timeOf(list[0]),
    // Newest first, like the column.
    forks: [...(i === 0 ? forksFrom.get(null) ?? [] : []), ...list.flatMap((x) => forksFrom.get(x.id) ?? [])]
      .sort((a, b) => byTime(b.line[b.line.length - 1], a.line[a.line.length - 1])),
  }));
  return { lineage: mine, days: byDay(rows.reverse(), (row) => row.versions[0]), ghost };
}

/** `rows` under day headers, in the order given, by the day `start` of each falls on. */
export function byDay<T>(rows: T[], start: (row: T) => Version): { day: number; rows: T[] }[] {
  const days: { day: number; rows: T[] }[] = [];
  for (const row of rows) {
    const day = startOfDay(new Date(timeOf(start(row))));
    if (days.length === 0 || days[days.length - 1].day !== day) days.push({ day, rows: [] });
    days[days.length - 1].rows.push(row);
  }
  return days;
}

/** Checkpoints in keyboard order: top to bottom, newest first within a row, open forks included. */
export function navOrder(layout: SessionsLayout, openForks: ReadonlySet<number>): number[] {
  const out: number[] = [];
  const add = (list: Version[]): void => {
    for (const version of [...list].reverse()) if (!version.retired) out.push(version.id);
  };
  for (const day of layout.days) {
    for (const row of day.rows) {
      add(row.versions);
      for (const fork of row.forks) {
        if (openForks.has(fork.root.id)) for (const session of [...fork.sessions].reverse()) add(session);
      }
    }
  }
  return out;
}

/**
 * The unrecorded changes after `unrecorded-changes` for `gameId`: a due time sets its entry,
 * `null` drops it. Nothing while the list hasn't been read; reading it gets the change too.
 */
export function withUnrecorded(entries: [number, string][] | undefined, gameId: number, dueAt: string | null): [number, string][] | undefined {
  if (entries === undefined) return undefined;
  const rest = entries.filter(([id]) => id !== gameId);
  return dueAt === null ? rest : [...rest, [gameId, dueAt]];
}

/** Whole minutes until `due`, 0 under a minute (or past it). */
export function minutesUntil(due: Date, now: Date): number {
  const ms = due.getTime() - now.getTime();
  return ms < MINUTE ? 0 : Math.round(ms / MINUTE);
}

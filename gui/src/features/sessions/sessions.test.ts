import { describe, expect, it } from 'vitest';
import { makeVersion, naive } from '@/ipc/builders';
import type { Version } from '@/ipc/bindings/Version';
import {
  dayLabel,
  durationParts,
  layoutSessions,
  lineage,
  minutesUntil,
  navOrder,
  playedMs,
  sessionGapMs,
  splitSessions,
  totalPlayedMs,
  trackPosition,
  trackWidth,
  withUnrecorded,
} from './sessions';

const MIN = 60_000;
const T0 = Date.UTC(2026, 9, 7, 18, 0);

/** A version `minutes` after T0. */
function v(id: number, parent: number | null, minutes: number, extra: Partial<Version> = {}): Version {
  return makeVersion({ id, parent_id: parent, created_at: naive(new Date(T0 + minutes * MIN)), ...extra });
}

/** A straight line of versions at the given minutes, ids from `first`. */
function line(first: number, parent: number | null, minutes: number[], extra: Partial<Version> = {}): Version[] {
  return minutes.map((m, i) => v(first + i, i === 0 ? parent : first + i - 1, m, extra));
}

const GAP = 40 * MIN;
/** Minutes in `h` hours. */
const day = (h: number): number => h * 60;
const ids = (list: Version[]): number[] => list.map((x) => x.id);

describe('sessionGapMs', () => {
  it('is 40 minutes, or 2.5 cooldowns when that is longer', () => {
    expect(sessionGapMs(5)).toBe(40 * MIN);
    expect(sessionGapMs(16)).toBe(40 * MIN);
    expect(sessionGapMs(20)).toBe(50 * MIN);
  });
});

describe('lineage', () => {
  it('follows parents from the active version back to the root, oldest first', () => {
    const versions = [...line(1, null, [0, 10, 20]), v(4, 2, 30), v(5, 4, 40)];
    expect(ids(lineage(versions, 5))).toEqual([1, 2, 4, 5]);
    expect(ids(lineage(versions, 3))).toEqual([1, 2, 3]);
  });

  it('is empty without an active version', () => {
    expect(lineage(line(1, null, [0, 10]), null)).toEqual([]);
  });
});

describe('splitSessions', () => {
  it('splits on gaps longer than the threshold', () => {
    const list = line(1, null, [0, 10, 30, 71, 80, 200]);
    expect(splitSessions(list, GAP).map(ids)).toEqual([[1, 2, 3], [4, 5], [6]]);
  });

  it('keeps a gap equal to the threshold in the session', () => {
    expect(splitSessions(line(1, null, [0, 40]), GAP).map(ids)).toEqual([[1, 2]]);
  });

  it('splits when the device changes', () => {
    const list = [v(1, null, 0, { origin_device: 'pc' }), v(2, 1, 5, { origin_device: 'pc' }), v(3, 2, 10, { origin_device: 'deck' })];
    expect(splitSessions(list, GAP).map(ids)).toEqual([[1, 2], [3]]);
  });

  it('sorts by time first', () => {
    const list = line(1, null, [0, 10, 20]);
    expect(splitSessions([list[2], list[0], list[1]], GAP).map(ids)).toEqual([[1, 2, 3]]);
  });
});

describe('playedMs', () => {
  it('sums the in-session gaps to each parent', () => {
    // 0→10→30 is 30 min of play; 30→90 is a break; 90→100 is 10 more.
    const versions = line(1, null, [0, 10, 30, 90, 100]);
    expect(playedMs(versions, versions, GAP)).toBe(40 * MIN);
  });

  it('counts a branch from its own fork point', () => {
    const versions = [...line(1, null, [0, 10]), v(3, 1, 20)];
    expect(playedMs([versions[2]], versions, GAP)).toBe(20 * MIN);
  });

  it('counts every timeline once for the header, however far back you rewound', () => {
    // Rewound to 2: 3─4 and 5─6 are other timelines, each played once.
    const versions = [...line(1, null, [0, 10]), ...line(3, 2, [30, 60]), ...line(5, 2, [day(48), day(48) + 30])];
    expect(totalPlayedMs(versions, GAP)).toBe((10 + 20 + 30 + 30) * MIN);
    expect(splitSessions(versions, GAP)).toHaveLength(2);
  });

  it('counts the minutes before a rewind once', () => {
    // 1 at 0, 2 at 10; rewound to 1, then 3 at 20: twenty minutes played, not thirty.
    const versions = [...line(1, null, [0, 10]), ...line(3, 1, [20])];
    expect(totalPlayedMs(versions, GAP)).toBe(20 * MIN);
  });
});

describe('durationParts', () => {
  it('rounds to minutes', () => {
    expect(durationParts(200 * MIN + 29_000)).toEqual({ hours: 3, minutes: 20 });
    expect(durationParts(45 * MIN)).toEqual({ hours: 0, minutes: 45 });
    expect(durationParts(20_000)).toEqual({ hours: 0, minutes: 0 });
  });
});

describe('dayLabel', () => {
  const words = { today: 'Today', yesterday: 'Yesterday' };
  const now = new Date(2026, 9, 7, 21, 0); // Wednesday

  it('names today and yesterday', () => {
    expect(dayLabel(new Date(2026, 9, 7, 0, 5), now, 'en', words)).toBe('Today');
    expect(dayLabel(new Date(2026, 9, 6, 23, 59), now, 'en', words)).toBe('Yesterday');
  });

  it('uses the weekday within the last week, then the date', () => {
    expect(dayLabel(new Date(2026, 9, 3, 12), now, 'en', words)).toBe('Saturday');
    expect(dayLabel(new Date(2026, 9, 1, 12), now, 'en', words)).toBe('Thursday');
    expect(dayLabel(new Date(2026, 8, 30, 12), now, 'en', words)).toBe('Wed, Sep 30');
  });

  it('is localized', () => {
    expect(dayLabel(new Date(2026, 9, 3, 12), now, 'es', words)).toBe('sábado');
  });
});

describe('layoutSessions', () => {

  it('groups your line into sessions under day headers, newest first', () => {
    const versions = [...line(1, null, [0, 10]), ...line(3, 2, [day(24), day(24) + 10])];
    const layout = layoutSessions({ versions, activeId: 4, gapMs: GAP });
    expect(layout.days.map((d) => d.rows.map((r) => ids(r.versions)))).toEqual([[[3, 4]], [[1, 2]]]);
    expect(layout.days[0].rows[0].durationMs).toBe(10 * MIN);
  });

  it('folds other timelines into forks under the session they leave from', () => {
    // 1─2─3─4 is your line; 5─6 left from 2, later.
    const versions = [...line(1, null, [0, 10, 20, 30]), ...line(5, 2, [200, 210])];
    const layout = layoutSessions({ versions, activeId: 4, gapMs: GAP });
    const rows = layout.days.flatMap((d) => d.rows);
    expect(rows.map((r) => ids(r.versions))).toEqual([[1, 2, 3, 4]]);
    const [fork] = rows[0].forks;
    expect(fork.from?.id).toBe(2);
    expect(ids(fork.line)).toEqual([5, 6]);
    expect(fork.sessions.map(ids)).toEqual([[5, 6]]);
  });

  it('shows newer checkpoints after a rewind as a fork from where you are', () => {
    const versions = line(1, null, [0, 10, 20]);
    const layout = layoutSessions({ versions, activeId: 2, gapMs: GAP });
    const row = layout.days[0].rows[0];
    expect(ids(row.versions)).toEqual([1, 2]);
    expect(row.forks.map((f) => [f.from?.id, ids(f.line)])).toEqual([[2, [3]]]);
  });

  it('keeps nested branches inside their fork', () => {
    const versions = [...line(1, null, [0, 10]), ...line(3, 1, [20, 30]), v(5, 3, 40)];
    const layout = layoutSessions({ versions, activeId: 2, gapMs: GAP });
    const forks = layout.days[0].rows[0].forks;
    expect(forks.map((f) => ids(f.line))).toEqual([[3, 4, 5]]);
  });

  it('pulls the pending progress out of the forks into the ghost', () => {
    const versions = [...line(1, null, [0, 10]), ...line(3, 2, [100, 110], { origin_device: 'deck' })];
    const layout = layoutSessions({ versions, activeId: 2, gapMs: GAP, pendingId: 4 });
    expect(layout.days[0].rows[0].forks).toEqual([]);
    expect(ids(layout.ghost ?? [])).toEqual([3, 4]);
  });

  it('attaches unrelated roots to the oldest session', () => {
    const versions = [...line(1, null, [0, 10]), ...line(3, null, [100, 110]), ...line(5, 2, [day(24)])];
    const layout = layoutSessions({ versions, activeId: 5, gapMs: GAP });
    const lastDay = layout.days[layout.days.length - 1];
    const oldest = lastDay.rows[lastDay.rows.length - 1];
    expect(oldest.forks.map((f) => [f.from?.id ?? null, ids(f.line)])).toEqual([[null, [3, 4]]]);
  });

  it('marks the fork holding the newest checkpoint, only when it is off your line', () => {
    // 1─2 is your line after a rewind; 3─4 left from 2 on Thursday, 5─6 today.
    const versions = [...line(1, null, [0, 10]), ...line(3, 2, [30, 60]), ...line(5, 2, [day(48), day(48) + 30])];
    const forks = (activeId: number): [number, boolean][] =>
      layoutSessions({ versions, activeId, gapMs: GAP }).days.flatMap((d) => d.rows.flatMap((r) => r.forks.map((f): [number, boolean] => [f.root.id, f.latest])));
    expect(forks(2)).toEqual([[5, true], [3, false]]);
    expect(forks(6)).toEqual([[3, false]]);
  });

  it('orders the forks leaving a row newest first, wherever on the row they leave', () => {
    const versions = [...line(1, null, [0, 10, 20]), ...line(4, 1, [day(24)]), ...line(5, 3, [100]), ...line(6, 2, [day(48)])];
    const layout = layoutSessions({ versions, activeId: 3, gapMs: GAP });
    expect(layout.days[0].rows[0].forks.map((f) => f.root.id)).toEqual([6, 4, 5]);
  });

  it('falls back to the newest version when nothing is active', () => {
    const versions = line(1, null, [0, 10]);
    expect(ids(layoutSessions({ versions, activeId: null, gapMs: GAP }).lineage)).toEqual([1, 2]);
  });

  it('is empty for no versions', () => {
    expect(layoutSessions({ versions: [], activeId: null, gapMs: GAP }).days).toEqual([]);
  });
});

describe('navOrder', () => {
  it('goes top to bottom, newest first in each row, through open forks, skipping cleaned up ones', () => {
    const versions = [
      ...line(1, null, [0, 10, 20], {}),
      ...line(4, 2, [200, 210]),
      ...line(6, 3, [day(24), day(24) + 5]),
    ];
    versions[0] = { ...versions[0], retired: true };
    const layout = layoutSessions({ versions, activeId: 7, gapMs: GAP });
    const forkRoot = layout.days[1].rows[0].forks[0].root.id;
    expect(navOrder(layout, new Set())).toEqual([7, 6, 3, 2]);
    expect(navOrder(layout, new Set([forkRoot]))).toEqual([7, 6, 3, 2, 5, 4]);
  });
});

describe('trackWidth', () => {
  it('grows with the session, from 40 to 380 px', () => {
    expect(trackWidth(0)).toBe(40);
    expect(trackWidth(120 * MIN)).toBe(210);
    expect(trackWidth(24 * 60 * MIN)).toBe(380);
  });
});

describe('trackPosition', () => {
  it('places checkpoints by time along the session, a lone one in the middle', () => {
    const session = line(1, null, [0, 10, 40]);
    expect(session.map((x) => trackPosition(x, session))).toEqual([0, 0.25, 1]);
    expect(trackPosition(session[0], [session[0]])).toBe(0.5);
  });
});

describe('withUnrecorded', () => {
  it('sets, updates and clears a game’s entry', () => {
    const set = withUnrecorded([[2, '2026-10-10T12:00:00']], 1, '2026-10-10T12:30:00');
    expect(set).toEqual([[2, '2026-10-10T12:00:00'], [1, '2026-10-10T12:30:00']]);
    expect(withUnrecorded(set, 1, '2026-10-10T12:31:00')).toEqual([[2, '2026-10-10T12:00:00'], [1, '2026-10-10T12:31:00']]);
    expect(withUnrecorded(set, 1, null)).toEqual([[2, '2026-10-10T12:00:00']]);
    expect(withUnrecorded([], 1, null)).toEqual([]);
  });

  it('leaves a list that hasn’t been read alone', () => {
    expect(withUnrecorded(undefined, 1, '2026-10-10T12:30:00')).toBeUndefined();
  });
});

describe('minutesUntil', () => {
  it('rounds to whole minutes, 0 under a minute or past due', () => {
    expect(minutesUntil(new Date(T0 + 12 * MIN + 10_000), new Date(T0))).toBe(12);
    expect(minutesUntil(new Date(T0 + 59_000), new Date(T0))).toBe(0);
    expect(minutesUntil(new Date(T0 - MIN), new Date(T0))).toBe(0);
  });
});

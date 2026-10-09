import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { on, signal, type OverlayShow } from '@/ipc/events';
import { formatRelative } from '@/shared/i18n/dates';
import { en } from '@/shared/i18n/en';
import { es } from '@/shared/i18n/es';
import { accentVars } from '@/shared/tokens/accent';

const MESSAGES = { en, es };
/** The text block's widest: the popup is at most 300px, less the padding and the tile. */
const TEXT_MAX = 300 - 2 * 8 - 32;
/** The container's exit animations (`overlay.css`): the fold's sink and the fades. */
const EXITS = new Set(['sink', 'fadeOut']);
/** Each kind's look in `overlay.css`, after the mock's names. */
const LOOKS: Record<OverlayShow['kind'], string> = {
  checkpoint: 'k-checkpoint',
  newTimeline: 'k-timeline',
  kept: 'k-pinned',
  sessionStart: 'k-recording',
  deviceAhead: 'k-ahead',
  captureFailed: 'k-failed',
};
/** The badge's glyph: a cross for the error, a bang for the warning. */
const BADGES: Partial<Record<OverlayShow['kind'], string>> = {
  captureFailed: 'M4.5 4.5l5 5M9.5 4.5l-5 5',
  deviceAhead: 'M7 3.2v4.6M7 10.6v.2',
};

/** The checkpoint number the popup shows, if its kind has one. */
function numberOf(show: OverlayShow): number | null {
  switch (show.kind) {
    case 'checkpoint':
    case 'newTimeline':
    case 'kept':
      return show.version;
    case 'sessionStart':
    case 'deviceAhead':
    case 'captureFailed':
      return null;
  }
}

/** The popup's two lines. */
function linesOf(show: OverlayShow): [string, string] {
  const t = MESSAGES[show.language].overlay;
  switch (show.kind) {
    case 'checkpoint':
      return [t.checkpoint, show.title];
    case 'newTimeline':
      return [
        t.newTimeline,
        show.from.tag !== null ? t.branchedOffNamed(show.from.tag) : t.branchedOffNumber(show.from.version_number),
      ];
    case 'kept':
      return [t.kept, show.name !== null ? t.keptNamed(show.name) : t.renameLater];
    case 'sessionStart':
      return [
        t.sessionStart,
        show.lastAt === null ? t.noCheckpointsYet : t.lastCheckpoint(formatRelative(new Date(show.lastAt), show.language)),
      ];
    case 'deviceAhead':
      return [t.ahead(show.device), t.aheadDetail(show.count)];
    case 'captureFailed':
      return [t.notSaved, `${show.reason === 'disk_full' ? t.diskFull : t.couldntWrite} · ${t.untouched}`];
  }
}

interface Shown {
  show: OverlayShow;
  /** The popup's first id: a swap keeps it, so only a new popup replays the entry. */
  key: number;
  /** The number the roll starts from: the one the swapped popup showed, or n - 1. */
  from: number;
  /** Swaps so far: the key that remounts, and so replays, what a swap animates. */
  swaps: number;
  /** `out`: folds into the mark and sinks. `gone`: the game left the front, a quick fade. */
  phase: 'in' | 'out' | 'gone';
}

/** The whole overlay page: at most one popup. Rust decides what shows and when it leaves; this
 * plays it and says when the exit has ended (`overlay-done`), so Rust can hide the window. */
export function Overlay(): ReactElement | null {
  const [shown, setShown] = useState<Shown | null>(null);
  const currentRef = useRef<Shown | null>(null);

  useEffect(() => {
    const set = (next: Shown | null): void => {
      currentRef.current = next;
      setShown(next);
    };
    let live = true;
    let unlisten: UnlistenFn[] = [];
    void Promise.all([
      on('overlay-show', (show) => {
        const prev = currentRef.current;
        const before = (numberOf(show) ?? 0) - 1;
        if (show.swap && prev?.phase === 'in') {
          const from = numberOf(prev.show) ?? before;
          set({ show, key: prev.key, from, swaps: prev.swaps + 1, phase: 'in' });
        } else {
          set({ show, key: show.id, from: before, swaps: 0, phase: 'in' });
        }
      }),
      on('overlay-leave', ({ id, fast }) => {
        const prev = currentRef.current;
        if (prev?.show.id !== id) {
          // Nothing of that popup is left to animate.
          void signal('overlay-done', id);
          return;
        }
        set({ ...prev, phase: fast ? 'gone' : 'out' });
      }),
    ]).then((fns) => {
      unlisten = fns;
      if (!live) fns.forEach((fn) => { fn(); });
      else void signal('overlay-ready', null);
    });
    return () => {
      live = false;
      unlisten.forEach((fn) => { fn(); });
    };
  }, []);

  if (!shown) return null;
  const exited = (): void => {
    currentRef.current = null;
    setShown(null);
    void signal('overlay-done', shown.show.id);
  };
  return <Popup key={shown.key} shown={shown} onExited={exited} />;
}

function Popup({ shown, onExited }: { shown: Shown; onExited: () => void }): ReactElement {
  const { show, from, swaps, phase } = shown;
  const [l1, l2] = linesOf(show);
  const n = numberOf(show);
  const badge = BADGES[show.kind];
  const txtRef = useRef<HTMLDivElement>(null);
  const linesRef = useRef<HTMLDivElement>(null);
  // The text block animates its width, so it needs a number: the text's own, before paint.
  useLayoutEffect(() => {
    if (txtRef.current && linesRef.current) {
      txtRef.current.style.width = `${Math.min(linesRef.current.offsetWidth, TEXT_MAX).toString()}px`;
    }
  }, [l1, l2, n]);

  const className = [
    'ov',
    LOOKS[show.kind],
    `c-${show.corner}`,
    phase === 'in' ? 'enter' : phase,
    swaps > 0 && 'swap',
    show.quiet && 'quiet',
    show.motion === 'reduced' && 'rm',
  ].filter(Boolean).join(' ');
  const style = {
    ...accentVars(show.accentHue),
    // The new timeline's branch colour: the accent's hue turned 155° (`c.lane` in the mock).
    '--lane': `oklch(80% 0.13 ${((show.accentHue + 155) % 360).toString()})`,
    '--dur': `${show.durationMs.toString()}ms`,
  } as CSSProperties;

  return (
    <div
      className={className}
      style={style}
      role="status"
      aria-live="polite"
      onAnimationEnd={(e) => {
        // The exit is the container's own animation; the others are its children's. Its entry
        // can end after a leave arrived, and isn't the exit.
        if (phase !== 'in' && e.target === e.currentTarget && EXITS.has(e.animationName)) onExited();
      }}
    >
      <div className="tile" key={`tile${swaps.toString()}`}>
        <svg className="mk" width="32" height="32" viewBox="0 0 64 64" aria-hidden="true">
          <path className="trunk" pathLength={1} d="M10 32 L26 32" />
          <path className="up" pathLength={1} d="M26 32 C 36 32 40 18 52 18" />
          <path className="dn" pathLength={1} d="M26 32 C 36 32 40 46 52 46" />
          <circle className="burst" cx="52" cy="18" r="4" />
          <circle className="end eu" cx="52" cy="18" r="3.4" />
          <circle className="end" cx="52" cy="46" r="3.4" />
          <circle className="lvr" cx="26" cy="32" r="6" />
          <circle className="node" cx="26" cy="32" r="6" />
          <path className="dia" d="M26 23 L35 32 L26 41 L17 32 Z" />
        </svg>
        <span className="ring" />
        {badge !== undefined && (
          <span className="badge">
            <svg width="10" height="10" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden="true">
              <path d={badge} />
            </svg>
          </span>
        )}
      </div>
      {!show.quiet && (
        <div className="txt" ref={txtRef}>
          <div className="lines" key={swaps} ref={linesRef}>
            <div className="l1">
              <span>{l1}</span>
              {n !== null && (
                <span className="num">
                  {show.kind === 'kept' ? (
                    // Kept is the number it already had: nothing rolls.
                    <span>#{n}</span>
                  ) : (
                    <span className={swaps === 0 ? 'roll late' : 'roll'}>
                      <span>#{from}</span>
                      <span>#{n}</span>
                    </span>
                  )}
                </span>
              )}
            </div>
            <div className="l2">{l2}</div>
          </div>
        </div>
      )}
      <span className="life" key={`life${swaps.toString()}`} />
    </div>
  );
}

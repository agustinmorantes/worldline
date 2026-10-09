import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactElement } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { on, signal, type OverlayShow } from '@/ipc/events';
import { en } from '@/shared/i18n/en';
import { es } from '@/shared/i18n/es';
import { accentVars } from '@/shared/tokens/accent';

const MESSAGES = { en, es };
/** The text block's widest: the popup is at most 300px, less the padding and the tile. */
const TEXT_MAX = 300 - 2 * 8 - 32;
/** The container's exit animations (`overlay.css`): the fold's sink and the fades. */
const EXITS = new Set(['sink', 'fadeOut']);

interface Shown {
  show: OverlayShow;
  /** The popup's first id: a swap keeps it, so only a new popup replays the entry. */
  key: number;
  /** The number the roll starts from: the previous checkpoint's, or n - 1. */
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
        if (show.swap && prev?.phase === 'in') {
          set({ show, key: prev.key, from: prev.show.version, swaps: prev.swaps + 1, phase: 'in' });
        } else {
          set({ show, key: show.id, from: show.version - 1, swaps: 0, phase: 'in' });
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
  const t = MESSAGES[show.language];
  const txtRef = useRef<HTMLDivElement>(null);
  const linesRef = useRef<HTMLDivElement>(null);
  // The text block animates its width, so it needs a number: the text's own, before paint.
  useLayoutEffect(() => {
    if (txtRef.current && linesRef.current) {
      txtRef.current.style.width = `${Math.min(linesRef.current.offsetWidth, TEXT_MAX).toString()}px`;
    }
  }, [show.title, show.version, show.language]);

  const className = [
    'ov',
    `c-${show.corner}`,
    phase === 'in' ? 'enter' : phase,
    swaps > 0 && 'swap',
    show.quiet && 'quiet',
    show.motion === 'reduced' && 'rm',
  ].filter(Boolean).join(' ');
  const style = { ...accentVars(show.accentHue), '--dur': `${show.durationMs.toString()}ms` } as CSSProperties;

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
          <circle className="end" cx="52" cy="18" r="3.4" />
          <circle className="end" cx="52" cy="46" r="3.4" />
          <circle className="node" cx="26" cy="32" r="6" />
        </svg>
        <span className="ring" />
      </div>
      {!show.quiet && (
        <div className="txt" ref={txtRef}>
          <div className="lines" key={swaps} ref={linesRef}>
            <div className="l1">
              <span>{t.overlay.checkpoint}</span>
              <span className="num">
                <span className={swaps === 0 ? 'roll late' : 'roll'}>
                  <span>#{from}</span>
                  <span>#{show.version}</span>
                </span>
              </span>
            </div>
            <div className="l2">{show.title}</div>
          </div>
        </div>
      )}
      <span className="life" key={`life${swaps.toString()}`} />
    </div>
  );
}

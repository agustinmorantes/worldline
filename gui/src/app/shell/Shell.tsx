// The app frame: one translucent surface for the title bar and sidebar over a glow in the
// current game's accent, and the main pane. Navigating between sidebar entries folds the pane
// into the old entry's icon and unfolds it from the new one, while the game icon flies
// between its sidebar row and the game header.
import { type ReactElement, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useRouter } from '@tanstack/react-router';
import { motion, useAnimationControls } from 'motion/react';
import { useGames } from '@/features/library/queries';
import { GameIcon } from '@/features/library/GameIcon';
import { anchorKey, type FlyingGame, type NavTarget, ShellNavContext } from '@/features/library/shell-nav';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { accentVars, BRAND_HUE } from '@/shared/tokens/accent';

/** How long the pane takes to fold into the old icon before the route changes. */
const FOLD_MS = 300;
/** How long the unfold runs after the route changes. */
const UNFOLD_MS = 620;

function targetOf(pathname: string): NavTarget | null {
  const game = /^\/game\/(\d+)/.exec(pathname);
  if (game) return { view: 'game', gameId: Number(game[1]) };
  if (pathname.startsWith('/settings')) return { view: 'settings' };
  if (pathname.startsWith('/add')) return { view: 'add' };
  return null;
}

function pathOf(target: NavTarget): string {
  switch (target.view) {
    case 'game': return `/game/${target.gameId.toString()}`;
    case 'add': return '/add';
    case 'settings': return '/settings';
  }
}

const sameTarget = (a: NavTarget | null, b: NavTarget | null): boolean =>
  a !== null && b !== null && anchorKey(a) === anchorKey(b);

function anchorRect(target: NavTarget | null): DOMRect | null {
  if (target === null) return null;
  return document.querySelector(`[data-nav-anchor="${anchorKey(target)}"]`)?.getBoundingClientRect() ?? null;
}

function dockRect(gameId: number): DOMRect | null {
  return document.querySelector(`[data-icon-dock="${gameId.toString()}"]`)?.getBoundingClientRect() ?? null;
}

interface Box { left: number; top: number; width: number; height: number }

const boxOf = (rect: DOMRect): Box => ({ left: rect.left, top: rect.top, width: rect.width, height: rect.height });

/** Calls `found` with the rect `find` returns once it has rendered (up to a second). */
function whenRendered(find: () => DOMRect | null, found: (rect: DOMRect) => void): void {
  let frames = 0;
  const look = (): void => {
    const rect = find();
    if (rect !== null) found(rect);
    else if (++frames < 60) requestAnimationFrame(look);
  };
  requestAnimationFrame(look);
}

interface Sheet { key: number; from: Box; to: Box; opening: boolean }

type FlightKind = 'home' | 'out' | 'land';

interface Flight { key: number; game: FlyingGame; from: Box; to: Box; kind: FlightKind }

export function Shell({ titleBar, sidebar, children }: {
  titleBar: ReactNode;
  sidebar: ReactNode;
  children: ReactNode;
}): ReactElement {
  const router = useRouter();
  const { pathname } = useLocation();
  const { data: games } = useGames();
  const reduced = useMotionReduced();
  const stage = useAnimationControls();
  const mainRef = useRef<HTMLElement>(null);
  const timersRef = useRef<number[]>([]);
  const navigatingRef = useRef(false);
  const foldingRef = useRef(false);
  /** Bumped per navigation so a newer one supersedes the unfold of the last. */
  const navIdRef = useRef(0);
  const counterRef = useRef(0);

  const current = useMemo(() => targetOf(pathname), [pathname]);
  const [pending, setPending] = useState<NavTarget | null>(null);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [flights, setFlights] = useState<Flight[]>([]);
  const [origin, setOrigin] = useState(0);

  const currentGame = current?.view === 'game' ? games?.find((g) => g.id === current.gameId) : undefined;
  const hue = currentGame?.accent_hue ?? BRAND_HUE;

  useEffect(() => () => { timersRef.current.forEach((t) => { clearTimeout(t); }); }, []);

  const later = useCallback((ms: number, fn: () => void): void => {
    timersRef.current.push(window.setTimeout(fn, ms));
  }, []);

  const fly = useCallback((game: FlyingGame, from: Box, to: Box, kind: FlightKind): void => {
    const key = ++counterRef.current;
    // One flight home and one out at a time; several games can land together.
    setFlights((all) => [...all.filter((f) => kind === 'land' || f.kind !== kind), { key, game, from, to, kind }]);
    later(kind === 'home' ? 400 : 560, () => { setFlights((all) => all.filter((f) => f.key !== key)); });
  }, [later]);

  /** Flies a game's icon from its sidebar row into the header once the header has rendered. */
  const flyOut = useCallback((game: FlyingGame): void => {
    const from = anchorRect({ view: 'game', gameId: game.id });
    if (from === null) return;
    whenRendered(() => dockRect(game.id), (to) => { fly(game, boxOf(from), boxOf(to), 'out'); });
  }, [fly]);

  const land = useCallback((game: FlyingGame, from: DOMRect): void => {
    if (reduced) return;
    whenRendered(
      () => anchorRect({ view: 'game', gameId: game.id }),
      (to) => { fly(game, boxOf(from), boxOf(to), 'land'); },
    );
  }, [reduced, fly]);

  const go = useCallback((target: NavTarget): void => {
    // Clicks during the fold are dropped; during the unfold they start the next switch.
    if (foldingRef.current || sameTarget(target, current)) return;
    const navigate = (): void => { void router.navigate({ to: pathOf(target) }); };
    if (reduced) {
      navigate();
      return;
    }

    const main = mainRef.current?.getBoundingClientRect();
    const fromAnchor = anchorRect(current);
    const toAnchor = anchorRect(target);
    const id = ++navIdRef.current;
    navigatingRef.current = true;
    foldingRef.current = true;
    setPending(target);

    if (main !== undefined && fromAnchor !== null) {
      setOrigin(fromAnchor.top + fromAnchor.height / 2 - main.top);
      setSheet({ key: ++counterRef.current, from: boxOf(main), to: boxOf(fromAnchor), opening: false });
    }
    void stage.start({ opacity: 0, x: -28, scale: 0.97, transition: { duration: 0.2, ease: [0.5, 0, 0.9, 0.5] } });

    const leaving = current?.view === 'game' ? games?.find((g) => g.id === current.gameId) : undefined;
    const leavingDock = leaving !== undefined ? dockRect(leaving.id) : null;
    if (leaving !== undefined && leavingDock !== null && fromAnchor !== null) {
      fly(leaving, boxOf(leavingDock), boxOf(fromAnchor), 'home');
    }

    later(FOLD_MS, () => {
      foldingRef.current = false;
      navigate();
      if (main !== undefined && toAnchor !== null) {
        setOrigin(toAnchor.top + toAnchor.height / 2 - main.top);
        setSheet({ key: ++counterRef.current, from: boxOf(toAnchor), to: boxOf(main), opening: true });
      }
      stage.set({ opacity: 0, x: -20, scale: 1 });
      void stage.start({ opacity: 1, x: 0, transition: { duration: 0.36, ease, delay: 0.24 } });
      const arriving = target.view === 'game' ? games?.find((g) => g.id === target.gameId) : undefined;
      if (arriving !== undefined) flyOut(arriving);
      later(UNFOLD_MS, () => {
        if (navIdRef.current !== id) return;
        setSheet(null);
        setPending(null);
        navigatingRef.current = false;
      });
    });
  }, [current, router, reduced, stage, games, fly, flyOut, later]);

  // Navigation that did not come from the sidebar (links inside screens) just slides in.
  useEffect(() => {
    if (navigatingRef.current) return;
    if (reduced) {
      stage.set({ opacity: 1, x: 0, scale: 1 });
      return;
    }
    stage.set({ opacity: 0, x: -20, scale: 1 });
    void stage.start({ opacity: 1, x: 0, transition: { duration: 0.36, ease } });
  }, [pathname, reduced, stage]);

  const flying = useMemo(() => ({
    home: flights.find((f) => f.kind === 'home')?.game.id ?? null,
    out: flights.find((f) => f.kind === 'out')?.game.id ?? null,
    landing: flights.filter((f) => f.kind === 'land').map((f) => f.game.id),
  }), [flights]);

  const nav = useMemo(() => ({ current, pending, go, land, flying }), [current, pending, go, land, flying]);

  return (
    <ShellNavContext value={nav}>
      <div
        className="relative isolate flex h-screen w-screen flex-col overflow-hidden bg-bg text-tx"
        style={accentVars(hue)}
      >
        <div
          aria-hidden="true"
          className="pointer-events-none absolute -top-[264px] left-[60px] h-[540px] w-[760px] rounded-full blur-[90px] transition-[background-color,opacity] duration-[800ms,500ms]"
          style={{
            backgroundColor: 'var(--acc-glow)',
            opacity: `calc(var(--glow-opacity) * ${currentGame ? '1' : '0.45'})`,
          }}
        />
        {titleBar}
        <div className="relative flex min-h-0 flex-1">
          {sidebar}
          {/* Zero-width, so the corner sits on the sidebar's edge and follows it as it folds. */}
          <div className="relative w-0 shrink-0">
            <div
              aria-hidden="true"
              className="pointer-events-none absolute left-0 top-0 z-[5] size-3"
              style={{ background: 'radial-gradient(circle at 100% 100%, transparent 11.5px, var(--tint) 12px)' }}
            />
          </div>
          <main ref={mainRef} className="relative z-[6] min-w-0 flex-1 overflow-hidden rounded-tl-[12px]">
            <motion.div
              animate={stage}
              initial={false}
              className="size-full"
              style={{ transformOrigin: `0 ${origin.toString()}px` }}
            >
              {children}
            </motion.div>
          </main>
        </div>

        {sheet !== null && (
          <motion.div
            key={sheet.key}
            aria-hidden="true"
            className="pointer-events-none fixed z-[5] bg-white/[0.03] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.07)]"
            initial={{ ...sheet.from, borderRadius: sheet.opening ? 11 : 12, opacity: sheet.opening ? 0 : 1 }}
            animate={{
              ...sheet.to,
              borderRadius: sheet.opening ? [11, 72, 12] : [12, 72, 11],
              opacity: sheet.opening ? [0, 1, 0] : [1, 1, 0],
            }}
            transition={sheet.opening
              ? { duration: 0.52, ease, opacity: { duration: 0.52, times: [0, 0.75, 1] } }
              : { duration: 0.3, ease: [0.6, 0, 0.35, 1] }}
          />
        )}

        {flights.map((flight) => {
          const size = Math.max(flight.from.width, flight.to.width);
          return (
            <motion.div
              key={flight.key}
              aria-hidden="true"
              className="pointer-events-none fixed left-0 top-0 z-50 origin-top-left drop-shadow-[0_18px_40px_oklch(0%_0_0/0.5)]"
              initial={{ x: flight.from.left, y: flight.from.top, scale: flight.from.width / size }}
              animate={{ x: flight.to.left, y: flight.to.top, scale: flight.to.width / size }}
              transition={{ duration: flight.kind === 'home' ? 0.38 : 0.52, ease }}
            >
              <GameIcon
                title={flight.game.title}
                hue={flight.game.accent_hue}
                iconPath={flight.game.icon_path ?? undefined}
                size={size}
              />
            </motion.div>
          );
        })}
      </div>
    </ShellNavContext>
  );
}

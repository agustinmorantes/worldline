import { type KeyboardEvent, type MouseEvent, type ReactElement, type ReactNode, useCallback, useMemo, useState } from 'react';
import { Link } from '@tanstack/react-router';
import { AnimatePresence, motion, type Transition } from 'motion/react';
import { LoaderCircle, Plus, SlidersHorizontal } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { useAddRun } from '@/features/add-games/add-run';
import { SecondDeviceHint } from '@/features/onboarding/SecondDeviceHint';
import { formatRelative, type Messages, useLocale, useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { accentVars } from '@/shared/tokens/accent';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { fadeLabel, type Fold, useNarrow } from './fold';
import { GameIcon } from './GameIcon';
import { type GameStatus, type GameSyncView, gameStatus, matchesTitle, sortLibrary } from './library';
import { LibrarySearch } from './LibrarySearch';
import { useGames, useRunningGames, useSyncReport, useSyncSettings } from './queries';
import { anchorKey, type NavTarget, useShellNav } from './shell-nav';
import { SyncCard } from './SyncCard';
import { useNow } from './useNow';

/** The rail's width spring: a touch heavier than `spring`, as the panel is big. No overshoot, so the main pane doesn't wobble. */
const panelSpring: Transition = { type: 'spring', stiffness: 260, damping: 32, mass: 1 };

export function Sidebar(): ReactElement {
  const t = useT();
  const { data: games, isLoading, error } = useGames();
  const { data: settings } = useSyncSettings();
  const { data: report } = useSyncReport();
  const { data: running } = useRunningGames();
  const reduced = useMotionReduced();
  const { go } = useShellNav();
  const collapsed = useNarrow();
  // Labels leave the layout only once the rail has finished narrowing.
  const [folded, setFolded] = useState(collapsed);
  const [wasCollapsed, setWasCollapsed] = useState(collapsed);
  if (wasCollapsed !== collapsed) {
    setWasCollapsed(collapsed);
    setFolded(false);
  }
  const fold: Fold = { collapsed, hidden: collapsed && (reduced || folded), reduced };
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState('');
  const openSearch = useCallback(() => { setSearching(true); }, []);
  const closeSearch = (): void => {
    setSearching(false);
    setQuery('');
  };
  const sorted = useMemo(() => (games ? sortLibrary(games) : []), [games]);
  const syncOn = typeof settings?.remote_path === 'string' && settings.remote_path !== '';
  const notHere = report?.remote_games.filter((g) => !g.adopted).length ?? 0;
  const run = useAddRun();

  const syncView = (game: FullGame): GameSyncView => ({
    on: syncOn,
    reported: report !== undefined,
    status: report?.games.find((s) => s.game_uuid === game.uuid),
  });
  const rows = sorted.map((game) => ({ game, status: gameStatus(game, syncView(game), running?.includes(game.id) === true) }));
  const fromDevice = rows.find((r) => r.status.kind === 'new-from' || r.status.kind === 'played-too');
  const shown = rows.filter((r) => matchesTitle(r.game.title, query));
  const empty = query !== '' && shown.length === 0;

  // Typing while focus is in the sidebar starts a search with that key ("/" opens it empty; "?" is help).
  const typeToSearch = (event: KeyboardEvent): void => {
    if (event.key.length !== 1 || event.key === ' ' || event.key === '/' || event.key === '?' || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target instanceof HTMLElement && (event.target.isContentEditable || event.target.closest('input, textarea, select') !== null)) return;
    event.preventDefault();
    setQuery(event.key);
    setSearching(true);
  };

  return (
    <motion.aside
      aria-label={t.shell.library}
      // Above the main pane while the rail's search flyout is out over it.
      className={cn('relative flex w-[240px] shrink-0 flex-col rounded-br-[12px] bg-tint max-[1099px]:w-[72px]', searching && collapsed ? 'z-[7]' : 'z-[4]')}
      initial={false}
      animate={{ width: collapsed ? 72 : 240 }}
      transition={reduced ? { duration: 0 } : panelSpring}
      onAnimationComplete={() => { setFolded(collapsed); }}
      onKeyDown={typeToSearch}
    >
      {/* The open search field is taller than the label, so the list steps down to clear it. */}
      <div className={cn('relative px-6 pb-2 pt-5 transition-[padding] duration-200 ease-ez', searching && !collapsed && 'pb-4')}>
        <div className={cn('whitespace-nowrap text-[11px] font-bold uppercase tracking-[0.12em] text-tx3', fadeLabel({ ...fold, collapsed: collapsed || searching, hidden: false }), fold.hidden && 'invisible')}>
          {t.shell.library}
        </div>
        <LibrarySearch
          fold={fold}
          open={searching}
          query={query}
          empty={empty}
          onOpen={openSearch}
          onQuery={setQuery}
          onClose={closeSearch}
          onSubmit={() => { if (shown.length > 0) go({ view: 'game', gameId: shown[0].game.id }); }}
        />
      </div>
      <nav aria-label={t.shell.library} className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-3 [scrollbar-width:none]">
        {isLoading ? (
          <div className="flex flex-col gap-1">
            {[0, 1, 2].map((i) => (
              <div key={i} className="flex h-[52px] items-center gap-3 px-2.5">
                <span className="size-9 shrink-0 animate-pulse rounded-[11px] bg-el" />
                <span className={cn('h-3 w-24 animate-pulse rounded bg-el', fadeLabel(fold, 'hidden'))} />
              </div>
            ))}
          </div>
        ) : error ? (
          <p className={cn('px-3 py-2 text-xs text-err', fadeLabel(fold))}>{t.errors.loadFailed}</p>
        ) : (
          <>
            <ul className="relative flex flex-col gap-1">
              {/* Rows a search filters out shrink and fade; the rest glide up into their place. */}
              <AnimatePresence mode="popLayout">
                {shown.map(({ game, status }) => (
                  <motion.li
                    key={game.id}
                    layout="position"
                    initial={reduced ? false : { opacity: 0, x: -12 }}
                    animate={{ opacity: 1, x: 0, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.92, transition: { duration: reduced ? 0 : 0.2, ease } }}
                    transition={{ duration: 0.56, ease }}
                  >
                    <GameRow game={game} status={status} fold={fold} />
                  </motion.li>
                ))}
              </AnimatePresence>
            </ul>
            <AnimatePresence>
              {empty && !collapsed && (
                <motion.p
                  key="no-matches"
                  className="whitespace-nowrap px-3 py-2 text-xs text-tx3"
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: reduced ? 0 : 0.24, ease }}
                >
                  {t.shell.noMatches}
                </motion.p>
              )}
            </AnimatePresence>
          </>
        )}
      </nav>
      {/* Pinned below the scrolling list so it stays reachable in a long library. */}
      <div className="flex flex-col gap-2 px-3 pb-3">
        <NavRow
          target={{ view: 'add' }}
          to="/add"
          label={t.shell.addGames}
          icon={<Plus size={16} strokeWidth={2.4} />}
          className="outline-[1.5px] -outline-offset-[1.5px] outline-dashed outline-ln"
          badge={run?.running === true && run.current !== null
            ? {
                count: <LoaderCircle size={12} strokeWidth={3} className={cn(!reduced && 'animate-spin')} />,
                label: t.shell.addingBadge(run.added.size + run.failed.size, run.total, run.current),
              }
            : notHere > 0 ? { count: notHere, label: t.shell.inCloudBadge(notHere) } : undefined}
          progress={run?.running === true ? (run.added.size + run.failed.size) / run.total : undefined}
          fold={fold}
        />
        <NavRow target={{ view: 'settings' }} to="/settings" label={t.shell.settings} icon={<SlidersHorizontal size={16} strokeWidth={2.2} />} fold={fold} />
        <SyncCard fold={fold} />
      </div>
      {fromDevice !== undefined && 'device' in fromDevice.status && (
        <SecondDeviceHint anchor={anchorKey({ view: 'game', gameId: fromDevice.game.id })} device={fromDevice.status.device} />
      )}
    </motion.aside>
  );
}

/** The sliding highlight on the selected entry, with the accent bar at the sidebar's edge. */
function SelectionPill({ hidden }: { hidden: boolean }): ReactElement {
  return (
    <motion.span
      layoutId="sidebar-selection"
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 rounded-[14px] bg-white/[0.06] before:absolute before:-left-3 before:top-1/2 before:h-6 before:w-1 before:-translate-y-1/2 before:rounded-r before:bg-acc before:transition-colors before:duration-600"
      initial={false}
      animate={{ opacity: hidden ? 0 : 1 }}
      transition={{ layout: { type: 'spring', stiffness: 420, damping: 30 }, opacity: { duration: 0.15 } }}
    />
  );
}

function useNavClick(target: NavTarget): (event: MouseEvent) => void {
  const { go } = useShellNav();
  return (event) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey) return;
    event.preventDefault();
    go(target);
  };
}

function GameRow({ game, status, fold }: { game: FullGame; status: GameStatus; fold: Fold }): ReactElement {
  const { current, pending, flying } = useShellNav();
  const target: NavTarget = { view: 'game', gameId: game.id };
  const onClick = useNavClick(target);
  const selected = current?.view === 'game' && current.gameId === game.id;
  // The open game's icon lives in the header; the row keeps an empty socket for it.
  const away = selected || flying.home === game.id || flying.landing.includes(game.id);

  return (
    <Tooltip side="right" content={game.title}>
      <Link
        to="/game/$gameId"
        params={{ gameId: game.id.toString() }}
        onClick={onClick}
        aria-current={selected ? 'page' : undefined}
        className={`group relative flex h-[52px] items-center gap-3 rounded-[14px] px-2.5 transition-colors duration-200 hover:bg-white/[0.04]`}
      >
        {selected && <SelectionPill hidden={pending !== null} />}
        {/* Left-anchored, the icon holds still as the rail narrows, then glides the last 4 px to centre. */}
        <span className={cn('relative size-9 shrink-0 transition-[translate] duration-[350ms] ease-ez', fold.collapsed && '-translate-x-1')} data-nav-anchor={anchorKey(target)}>
          {away ? (
            <span
              aria-hidden="true"
              className="absolute inset-0 rounded-[11px] border-[1.5px] border-dashed transition-colors duration-300"
              style={{ borderColor: accentVars(game.accent_hue)['--acc-line'] }}
            />
          ) : (
            <GameIcon
              title={game.title}
              hue={game.accent_hue}
              iconPath={game.icon_path ?? undefined}
              iconRevision={game.icon_sha256 ?? undefined}
              className="transition-transform duration-300 ease-spring group-hover:scale-[1.08]"
            />
          )}
          {/* The narrow rail hides the status line, so the icon carries the live dot. */}
          {status.kind === 'recording' && (
            <span aria-hidden="true" className={cn('absolute -right-0.5 -top-0.5 size-2 rounded-full bg-live transition-opacity duration-300 [animation:blink_1.4s_ease-in-out_infinite]', !fold.collapsed && 'opacity-0')} />
          )}
        </span>
        <span className={cn('relative flex min-w-0 flex-col gap-[3px]', fadeLabel(fold))}>
          <span className={cn('truncate text-sm font-semibold', selected ? 'text-tx' : 'text-tx2')}>{game.title}</span>
          <StatusLine status={status} />
        </span>
      </Link>
    </Tooltip>
  );
}

function statusText(status: GameStatus, t: Messages, when: (date: Date) => string): string {
  switch (status.kind) {
    case 'recording': return t.gameStatus.recording;
    case 'new-from': return t.gameStatus.newFrom(status.device);
    case 'played-too': return t.gameStatus.playedToo(status.device);
    case 'not-in-cloud': return t.gameStatus.notInCloud;
    case 'no-saves': return t.gameStatus.noSaves;
    case 'last-played': return when(status.at);
  }
}

function StatusLine({ status }: { status: GameStatus }): ReactElement {
  const t = useT();
  const locale = useLocale();
  const now = useNow(60_000);
  const fromDevice = status.kind === 'new-from' || status.kind === 'played-too';
  const recording = status.kind === 'recording';
  const text = statusText(status, t, (date) => {
    const relative = formatRelative(date, locale, now);
    return relative.charAt(0).toLocaleUpperCase(locale) + relative.slice(1);
  });
  // "Recording" fades in when the game starts and back out when it stops.
  return (
    <AnimatePresence initial={false} mode="wait">
      <motion.span
        key={recording ? 'recording' : 'idle'}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.3 }}
        className={cn('flex items-center gap-1.5 truncate text-xs font-medium transition-colors duration-300', recording ? 'text-live' : fromDevice ? 'text-deck' : 'text-tx3')}
      >
        {recording && <span aria-hidden="true" className="size-[7px] shrink-0 rounded-full bg-live [animation:blink_1.4s_ease-in-out_infinite]" />}
        {fromDevice && <DeviceGlyph kind="handheld" />}
        <span className="truncate">{text}</span>
      </motion.span>
    </AnimatePresence>
  );
}

function NavRow({ target, to, label, icon, className, badge, progress, fold }: {
  target: NavTarget;
  to: '/add' | '/settings';
  label: string;
  icon: ReactNode;
  className?: string;
  badge?: { count: ReactNode; label: string };
  /** 0 to 1: a bar along the row's foot, while something it leads to is under way. */
  progress?: number;
  fold: Fold;
}): ReactElement {
  const { current, pending } = useShellNav();
  const onClick = useNavClick(target);
  const selected = current !== null && anchorKey(current) === anchorKey(target);

  return (
    <Tooltip side="right" content={badge === undefined ? label : <>{label}<span className="block font-medium opacity-70">{badge.label}</span></>}>
      <Link
        to={to}
        onClick={onClick}
        aria-current={selected ? 'page' : undefined}
        className={cn(
          `group relative flex h-11 items-center gap-3 rounded-xl px-3.5 text-sm font-semibold transition-colors duration-200 hover:bg-white/[0.05] hover:text-tx`,
          className,
          selected ? 'text-tx outline-transparent' : 'text-tx2',
        )}
      >
        {selected && <SelectionPill hidden={pending !== null} />}
        <span className={cn('relative flex transition-[translate] duration-[350ms] ease-ez', fold.collapsed && 'translate-x-0.5')} data-nav-anchor={anchorKey(target)}>{icon}</span>
        <span className={cn('relative min-w-0 flex-1 overflow-hidden whitespace-nowrap', fadeLabel(fold))}>{label}</span>
        {badge !== undefined && (
          <span
            // Pinned to the row's right edge, it rides in with the rail, then shrinks into the corner.
            className={cn(
              'absolute right-3.5 top-3 flex h-5 min-w-5 origin-top-right items-center justify-center rounded-full bg-tx2 px-1.5 text-xs font-bold text-bg transition-[translate,scale] duration-[350ms] ease-ez',
              fold.collapsed && 'translate-x-2.5 -translate-y-2 scale-[0.8]',
            )}
          >
            <span aria-hidden="true">{badge.count}</span>
            <span className="sr-only">{badge.label}</span>
          </span>
        )}
        {progress !== undefined && (
          <span aria-hidden="true" className="absolute inset-x-3.5 bottom-1.5 h-[3px] overflow-hidden rounded-full bg-el">
            <motion.span className="block h-full rounded-full bg-acc" initial={false}
              animate={{ width: `${(100 * progress).toString()}%` }} transition={{ duration: fold.reduced ? 0 : 0.3, ease }} />
          </span>
        )}
      </Link>
    </Tooltip>
  );
}

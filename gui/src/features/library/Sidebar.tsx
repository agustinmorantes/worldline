import { type MouseEvent, type ReactElement, type ReactNode, useMemo } from 'react';
import { Link } from '@tanstack/react-router';
import { motion } from 'motion/react';
import { Plus, SlidersHorizontal } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { formatRelative, type Messages, useLocale, useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { accentVars } from '@/shared/tokens/accent';
import { cn } from '@/shared/ui/utils';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { GameIcon } from './GameIcon';
import { type GameStatus, type GameSyncView, gameStatus, sortLibrary } from './library';
import { useGames, useSyncReport, useSyncSettings } from './queries';
import { anchorKey, type NavTarget, useShellNav } from './shell-nav';
import { SyncCard } from './SyncCard';
import { useNow } from './useNow';


export function Sidebar(): ReactElement {
  const t = useT();
  const { data: games, isLoading, error } = useGames();
  const { data: settings } = useSyncSettings();
  const { data: report } = useSyncReport();
  const reduced = useMotionReduced();
  const sorted = useMemo(() => (games ? sortLibrary(games) : []), [games]);
  const syncOn = typeof settings?.remote_path === 'string' && settings.remote_path !== '';
  const notHere = report?.remote_games.filter((g) => !g.adopted).length ?? 0;

  const syncView = (game: FullGame): GameSyncView => ({
    on: syncOn,
    reported: report !== undefined,
    status: report?.games.find((s) => s.game_uuid === game.uuid),
  });

  return (
    <aside
      aria-label={t.shell.library}
      className={`relative z-[4] flex w-[240px] shrink-0 flex-col rounded-br-[12px] bg-tint max-[1099px]:w-[72px]`}
    >
      <div className={`px-6 pb-2 pt-5 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3 max-[1099px]:invisible`}>
        {t.shell.library}
      </div>
      <nav aria-label={t.shell.library} className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 pb-3 [scrollbar-width:none]">
        {isLoading ? (
          <div className="flex flex-col gap-1">
            {[0, 1, 2].map((i) => (
              <div key={i} className="flex h-[52px] items-center gap-3 px-2.5">
                <span className="size-9 shrink-0 animate-pulse rounded-[11px] bg-el" />
                <span className={`h-3 w-24 animate-pulse rounded bg-el max-[1099px]:hidden`} />
              </div>
            ))}
          </div>
        ) : error ? (
          <p className={`px-3 py-2 text-xs text-err max-[1099px]:sr-only`}>{t.errors.loadFailed}</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {sorted.map((game) => (
              <motion.li
                key={game.id}
                layout="position"
                initial={reduced ? false : { opacity: 0, x: -12 }}
                animate={{ opacity: 1, x: 0 }}
                transition={{ duration: 0.56, ease }}
              >
                <GameRow game={game} status={gameStatus(game, syncView(game))} />
              </motion.li>
            ))}
          </ul>
        )}
        <NavRow
          id="tour-add-game"
          target={{ view: 'add' }}
          to="/import"
          label={t.shell.addGames}
          icon={<Plus size={16} strokeWidth={2.4} />}
          className="mt-2.5 outline-[1.5px] -outline-offset-[1.5px] outline-dashed outline-ln"
          badge={notHere > 0 ? { count: notHere, label: t.shell.inCloudBadge(notHere) } : undefined}
        />
      </nav>
      <div className="flex flex-col gap-2 px-3 pb-3">
        <NavRow target={{ view: 'settings' }} to="/settings" label={t.shell.settings} icon={<SlidersHorizontal size={16} strokeWidth={2.2} />} />
        <SyncCard />
      </div>
    </aside>
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

function GameRow({ game, status }: { game: FullGame; status: GameStatus }): ReactElement {
  const { current, pending, flying } = useShellNav();
  const target: NavTarget = { view: 'game', gameId: game.id };
  const onClick = useNavClick(target);
  const selected = current?.view === 'game' && current.gameId === game.id;
  // The open game's icon lives in the header; the row keeps an empty socket for it.
  const away = selected || flying.home === game.id;

  return (
    <Link
      to="/game/$gameId"
      params={{ gameId: game.id.toString() }}
      onClick={onClick}
      aria-current={selected ? 'page' : undefined}
      title={game.title}
      className={`group relative flex h-[52px] items-center gap-3 rounded-[14px] px-2.5 transition-colors duration-200 hover:bg-white/[0.04] max-[1099px]:justify-center max-[1099px]:px-0`}
    >
      {selected && <SelectionPill hidden={pending !== null} />}
      <span className="relative size-9 shrink-0" data-nav-anchor={anchorKey(target)}>
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
            className="transition-transform duration-300 ease-spring group-hover:scale-[1.08]"
          />
        )}
      </span>
      <span className={`relative flex min-w-0 flex-col gap-[3px] max-[1099px]:sr-only`}>
        <span className={cn('truncate text-sm font-semibold', selected ? 'text-tx' : 'text-tx2')}>{game.title}</span>
        <StatusLine status={status} />
      </span>
    </Link>
  );
}

function statusText(status: GameStatus, t: Messages, when: (date: Date) => string): string {
  switch (status.kind) {
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
  const text = statusText(status, t, (date) => {
    const relative = formatRelative(date, locale, now);
    return relative.charAt(0).toLocaleUpperCase(locale) + relative.slice(1);
  });
  return (
    <span className={cn('flex items-center gap-1.5 truncate text-xs font-medium transition-colors duration-300', fromDevice ? 'text-deck' : 'text-tx3')}>
      {fromDevice && <DeviceGlyph kind="handheld" />}
      {text}
    </span>
  );
}

function NavRow({ id, target, to, label, icon, className, badge }: {
  id?: string;
  target: NavTarget;
  to: '/import' | '/settings';
  label: string;
  icon: ReactNode;
  className?: string;
  badge?: { count: number; label: string };
}): ReactElement {
  const { current, pending } = useShellNav();
  const onClick = useNavClick(target);
  const selected = current !== null && anchorKey(current) === anchorKey(target);

  return (
    <Link
      id={id}
      to={to}
      onClick={onClick}
      aria-current={selected ? 'page' : undefined}
      title={label}
      className={cn(
        `group relative flex h-11 items-center gap-3 rounded-xl px-3.5 text-sm font-semibold transition-colors duration-200 hover:bg-white/[0.05] hover:text-tx max-[1099px]:justify-center max-[1099px]:px-0`,
        className,
        selected ? 'text-tx outline-transparent' : 'text-tx2',
      )}
    >
      {selected && <SelectionPill hidden={pending !== null} />}
      <span className="relative flex" data-nav-anchor={anchorKey(target)}>{icon}</span>
      <span className={`relative flex-1 max-[1099px]:sr-only`}>{label}</span>
      {badge !== undefined && (
        <span
          title={badge.label}
          className={`relative flex h-5 min-w-5 items-center justify-center rounded-full bg-tx2 px-1.5 text-xs font-bold text-bg max-[1099px]:absolute max-[1099px]:right-1 max-[1099px]:top-1 max-[1099px]:h-4 max-[1099px]:min-w-4 max-[1099px]:px-1 max-[1099px]:text-[10px]`}
        >
          <span aria-hidden="true">{badge.count}</span>
          <span className="sr-only">{badge.label}</span>
        </span>
      )}
    </Link>
  );
}

import type { ReactElement } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Cloud, CloudAlert, CloudOff, Monitor } from 'lucide-react';
import { reportError } from '@/shared/errors/report';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { ease, spring } from '@/shared/motion/motion';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { fadeLabel, type Fold } from './fold';
import { refreshSyncReport, useSyncReport, useSyncSettings } from './queries';
import { useShellNav } from './shell-nav';
import { isFolderUnreachable, useFolderUnreachable, useSyncRunning } from './sync-activity';
import { useNow } from './useNow';

type SyncState = 'synced' | 'syncing' | 'unavailable' | 'failed' | 'idle';

/** "This PC ── Cloud" at the bottom of the sidebar. Click to sync now. */
export function SyncCard({ fold }: { fold: Fold }): ReactElement | null {
  const t = useT();
  const locale = useLocale();
  const { data: settings } = useSyncSettings();
  const report = useSyncReport();
  const running = useSyncRunning();
  const unreachable = useFolderUnreachable();
  const now = useNow(30_000);
  const { go } = useShellNav();

  if (settings === undefined) return null;

  const card = 'w-full rounded-[14px] bg-white/[0.03] transition-[background-color,transform] duration-200 ease-spring hover:bg-white/[0.06] active:scale-[0.97]';
  // The rail centres the cloud icon; it glides the last 2 px as the card narrows.
  const glide = 'transition-[translate] duration-[350ms] ease-ez';

  if (settings.remote_path === null || settings.remote_path === '') {
    return (
      <button type="button" className={`${card} px-3.5 py-3`} onClick={() => { go({ view: 'settings' }); }}>
        <span className="flex items-center gap-2 text-[13px] font-semibold text-tx">
          <CloudOff size={16} aria-hidden="true" className={cn('shrink-0 text-tx2', glide, fold.collapsed && 'translate-x-0.5')} />
          <span className={cn('min-w-0 overflow-hidden whitespace-nowrap', fadeLabel(fold))}>{t.sync.turnOn}</span>
        </span>
        <Unfold fold={fold}>
          <span className="block pt-1.5 text-left text-xs text-tx3">{t.sync.turnOnHint}</span>
        </Unfold>
      </button>
    );
  }

  const syncing = running || report.isFetching;
  const unavailable = unreachable && !syncing;
  const state: SyncState = syncing
    ? 'syncing'
    : unavailable
      ? 'unavailable'
      : report.isError
        ? 'failed'
        : report.data !== undefined ? 'synced' : 'idle';
  const status = {
    syncing: t.sync.syncing,
    unavailable: t.sync.unavailable,
    failed: t.sync.syncFailed,
    synced: report.data === undefined ? '' : t.sync.synced(formatRelative(new Date(report.data.synced_at), locale, now)),
    idle: t.sync.notSynced,
  }[state];
  const warn = state === 'unavailable' || state === 'failed';

  const syncNow = (): void => {
    if (syncing) return;
    void refreshSyncReport().catch((error: unknown) => {
      // The card itself says when the folder is unavailable.
      if (!isFolderUnreachable(error)) reportError(t.sync.failed, error);
    });
  };

  return (
    <Tooltip side="right" content={state === 'unavailable' ? t.sync.unavailableHint : state === 'failed' ? t.sync.failedHint : t.sync.syncNow}>
      <button
        type="button"
        aria-label={`${t.sync.syncNow}. ${status}`}
        aria-busy={syncing}
        data-state={state}
        className={`${card} px-3.5 py-3`}
        onClick={syncNow}
      >
        <span className="flex items-center justify-end gap-2 text-xs font-semibold text-tx2">
          <span className={cn('flex min-w-0 flex-1 items-center gap-2 overflow-hidden whitespace-nowrap', fadeLabel(fold, 'hidden'))}>
            <Monitor size={15} aria-hidden="true" className="shrink-0" />
            <span>{t.sync.thisPc}</span>
            <span className="relative h-0.5 flex-1 rounded-sm bg-ln">
              {syncing && (
                <span className="absolute -top-0.5 size-1.5 rounded-full bg-tx shadow-[0_0_8px_var(--tx2)] [animation:travel_650ms_ease-in-out_infinite_alternate]" />
              )}
            </span>
            <span className="text-tx">{t.sync.cloud}</span>
          </span>
          <span className={cn('relative flex shrink-0', glide, fold.collapsed && '-translate-x-0.5')}>
            <SyncIcon state={state} reduced={fold.reduced} />
            {/* The rail hides the line, so the icon carries the travelling dot's blink. */}
            {syncing && (
              <span className={cn('absolute -right-0.5 -top-0.5 size-1.5 rounded-full bg-tx transition-opacity duration-300 [animation:blink_1.4s_ease-in-out_infinite]', !fold.collapsed && 'opacity-0')} />
            )}
          </span>
        </span>
        <Unfold fold={fold}>
          <span className="relative block pt-2">
            <AnimatePresence initial={false} mode="popLayout">
              {/* Keyed by state, not text: the clock ticking "Synced 2 min ago" doesn't slide. */}
              <motion.span
                key={state}
                className={cn('block truncate text-left text-xs transition-colors duration-300', warn ? 'text-warn' : 'text-tx3')}
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -6 }}
                transition={fold.reduced ? { duration: 0 } : { duration: 0.28, ease }}
              >
                {status}
              </motion.span>
            </AnimatePresence>
          </span>
        </Unfold>
      </button>
    </Tooltip>
  );
}

/** The second line of the card: it folds its height away with the rail instead of popping. */
function Unfold({ fold, children }: { fold: Fold; children: ReactElement }): ReactElement {
  return (
    <motion.span
      className={cn('block overflow-hidden', fold.hidden && 'hidden')}
      initial={false}
      animate={fold.collapsed ? { height: 0, opacity: 0 } : { height: 'auto', opacity: 1 }}
      transition={fold.reduced ? { duration: 0 } : { duration: 0.32, ease }}
    >
      {children}
    </motion.span>
  );
}

/** One icon per state. A new state crossfades in with a small pop; landing on "synced" draws the check. */
function SyncIcon({ state, reduced }: { state: SyncState; reduced: boolean }): ReactElement {
  const icon = {
    synced: <CloudCheckDrawn reduced={reduced} />,
    syncing: <Cloud size={16} aria-hidden="true" />,
    unavailable: <CloudOff size={16} aria-hidden="true" className="text-warn" />,
    failed: <CloudAlert size={16} aria-hidden="true" className="text-warn" />,
    idle: <Cloud size={16} aria-hidden="true" className="text-tx3" />,
  }[state];
  return (
    <AnimatePresence initial={false} mode="popLayout">
      <motion.span
        key={state}
        className="flex text-tx"
        initial={{ opacity: 0, scale: 0.8 }}
        animate={{ opacity: 1, scale: 1, transition: reduced ? { duration: 0 } : { opacity: { duration: 0.2 }, scale: spring } }}
        exit={{ opacity: 0, scale: 0.8, transition: { duration: reduced ? 0 : 0.15 } }}
      >
        {icon}
      </motion.span>
    </AnimatePresence>
  );
}

/**
 * Lucide's `cloud-check`, with the check as its own path so it can draw itself (short stroke
 * first) when a sync lands. It mounts with the "synced" state, so ticks and re-renders don't redraw it.
 */
function CloudCheckDrawn({ reduced }: { reduced: boolean }): ReactElement {
  return (
    <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="lucide-cloud-check">
      <path d="M5 17.743A7 7 0 1 1 15.71 10h1.79a4.5 4.5 0 0 1 1.5 8.742" />
      <motion.path
        d="M9 18l2.5 2.5L17 15"
        initial={{ pathLength: 0 }}
        animate={{ pathLength: 1 }}
        transition={reduced ? { duration: 0 } : { duration: 0.3, ease, delay: 0.12 }}
      />
    </svg>
  );
}

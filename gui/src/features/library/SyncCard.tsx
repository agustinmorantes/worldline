import type { ReactElement } from 'react';
import { Cloud, CloudOff, Monitor } from 'lucide-react';
import { reportError } from '@/shared/errors/report';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { useSyncReport, useSyncSettings } from './queries';
import { useShellNav } from './shell-nav';
import { useSyncRunning } from './sync-activity';
import { useNow } from './useNow';

/** "This PC ── Cloud" at the bottom of the sidebar. Click to sync now. */
export function SyncCard(): ReactElement | null {
  const t = useT();
  const locale = useLocale();
  const { data: settings } = useSyncSettings();
  const report = useSyncReport();
  const running = useSyncRunning();
  const now = useNow(30_000);
  const { go } = useShellNav();

  if (settings === undefined) return null;

  const card = 'w-full rounded-[14px] bg-white/[0.03] transition-[background-color,transform] duration-200 ease-spring hover:bg-white/[0.06] active:scale-[0.97]';

  if (settings.remote_path === null || settings.remote_path === '') {
    return (
      <button type="button" className={`${card} px-3.5 py-3 max-[1099px]:px-0`} onClick={() => { go({ view: 'settings' }); }}>
        <span className="flex items-center gap-2 text-[13px] font-semibold text-tx max-[1099px]:justify-center">
          <CloudOff size={16} aria-hidden="true" className="shrink-0 text-tx2" />
          <span className="max-[1099px]:sr-only">{t.sync.turnOn}</span>
        </span>
        <span className="mt-1.5 block text-xs text-tx3 max-[1099px]:hidden">{t.sync.turnOnHint}</span>
      </button>
    );
  }

  const syncing = running || report.isFetching;
  const status = syncing
    ? t.sync.syncing
    : report.data !== undefined
      ? t.sync.synced(formatRelative(new Date(report.data.synced_at), locale, now))
      : report.isError ? t.sync.unreachable : t.sync.notSynced;

  const syncNow = (): void => {
    if (syncing) return;
    void report.refetch().then((result) => {
      if (result.isError) reportError(t.sync.failed, result.error);
    });
  };

  return (
    <button
      type="button"
      title={t.sync.syncNow}
      aria-label={`${t.sync.syncNow}. ${status}`}
      aria-busy={syncing}
      className={`${card} px-3.5 py-3 max-[1099px]:px-0`}
      onClick={syncNow}
    >
      <span className="flex items-center gap-2 text-xs font-semibold text-tx2 max-[1099px]:justify-center">
        <Monitor size={15} aria-hidden="true" className="shrink-0 max-[1099px]:hidden" />
        <span className="max-[1099px]:hidden">{t.sync.thisPc}</span>
        <span className="relative h-0.5 flex-1 rounded-sm bg-ln max-[1099px]:hidden">
          {syncing && (
            <span className="absolute -top-0.5 size-1.5 rounded-full bg-tx shadow-[0_0_8px_var(--tx2)] [animation:travel_650ms_ease-in-out_infinite_alternate]" />
          )}
        </span>
        <span className="text-tx max-[1099px]:hidden">{t.sync.cloud}</span>
        <span className="relative flex text-tx">
          <Cloud size={16} aria-hidden="true" />
          {syncing && (
            <span className="absolute -right-0.5 -top-0.5 hidden size-1.5 rounded-full bg-tx [animation:blink_1.4s_ease-in-out_infinite] max-[1099px]:block" />
          )}
        </span>
      </span>
      <span className="mt-2 block truncate text-left text-xs text-tx3 max-[1099px]:hidden">{status}</span>
    </button>
  );
}

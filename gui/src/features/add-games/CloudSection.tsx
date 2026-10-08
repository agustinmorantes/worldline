// Games in the cloud that aren't on this PC: adding one adopts it, and its icon flies into the
// sidebar.
import { type ReactElement, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Cloud } from 'lucide-react';
import type { RemoteGameSummary } from '@/ipc/bindings/RemoteGameSummary';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import { adoptRemoteGame } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { GameIcon } from '@/features/library/GameIcon';
import { useShellNav } from '@/features/library/shell-nav';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { btn, eyebrow } from '@/shared/ui/classes';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { cn } from '@/shared/ui/utils';

export function CloudSection({ report }: { report: SyncReport }): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const waiting = report.remote_games.filter((g) => !g.adopted);

  return (
    <section aria-labelledby="add-cloud">
      <h2 id="add-cloud" className={cn(eyebrow, 'flex items-center gap-2 text-tx2')}>
        <Cloud size={16} strokeWidth={2} aria-hidden="true" />
        {t.addGames.inCloud}
      </h2>
      <ul className="mt-3 flex min-h-[92px] flex-wrap gap-4">
        <AnimatePresence initial={false} mode="popLayout">
          {waiting.map((game) => (
            <motion.li
              key={game.uuid}
              layout={!reduced}
              exit={reduced ? undefined : { opacity: 0, scale: 0.92, transition: { duration: 0.3, ease } }}
            >
              <CloudCard game={game} report={report} />
            </motion.li>
          ))}
          {waiting.length === 0 && (
            <motion.li
              key="empty"
              initial={reduced ? false : { opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ duration: 0.42, ease, delay: 0.2 }}
              className="flex h-[92px] items-center text-sm text-tx3"
            >
              {t.addGames.cloudEmpty}
            </motion.li>
          )}
        </AnimatePresence>
      </ul>
    </section>
  );
}

function CloudCard({ game, report }: { game: RemoteGameSummary; report: SyncReport }): ReactElement {
  const t = useT();
  const { land } = useShellNav();
  const [busy, setBusy] = useState(false);
  const iconRef = useRef<HTMLSpanElement>(null);
  const kind = report.devices.find((d) => d.name === game.source_device)?.kind ?? 'desktop';

  async function adopt(): Promise<void> {
    setBusy(true);
    try {
      const adopted = await adoptRemoteGame(game.uuid);
      const from = iconRef.current?.getBoundingClientRect();
      // Mark it adopted in place: re-syncing just to learn that would be slow.
      queryClient.setQueryData<SyncReport>(['sync-report'], (old) => old && {
        ...old,
        remote_games: old.remote_games.map((g) => (g.uuid === game.uuid ? { ...g, adopted: true } : g)),
      });
      await queryClient.invalidateQueries({ queryKey: ['games'] });
      if (from !== undefined) land(adopted, from);
    } catch (err) {
      reportError(t.addGames.failed.adopt, err);
      setBusy(false);
    }
  }

  return (
    <div className="flex h-[92px] min-w-[316px] max-w-[420px] items-center gap-3.5 rounded-[18px] bg-sf px-4">
      <span ref={iconRef} className="shrink-0"><GameIcon title={game.title} size={48} /></span>
      <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span className="truncate text-[15px] font-bold">{game.title}</span>
        <span className="flex items-center gap-1.5 truncate text-[12.5px] text-tx2">
          {game.source_device !== null && <span className="text-deck"><DeviceGlyph kind={kind} /></span>}
          {t.addGames.fromDevice(game.source_device, game.version_count)}
        </span>
        <span className={cn('text-[12.5px]', game.installed ? 'text-live' : 'text-tx3')}>
          {game.installed ? t.addGames.installedHere : t.addGames.notInstalledHere}
        </span>
      </span>
      <button
        type="button"
        className={btn}
        disabled={busy}
        aria-label={t.addGames.addTitle(game.title)}
        onClick={() => { void adopt(); }}
      >
        {t.addGames.add}
      </button>
    </div>
  );
}

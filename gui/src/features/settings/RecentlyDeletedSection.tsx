// Games and playthroughs deleted on any device, while they can still come back (ADR 0011). A row
// restored glides off toward the library; one deleted now sinks away. Hidden while it's empty.
import { type ReactElement, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Download, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import type { DeletedItem } from '@/ipc/bindings/DeletedItem';
import { exportDeletedGame, pickSavePath, purgeDeleted, recoverGame, recoverSlot } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { addFailedHint } from '@/features/add-games/add-games';
import { useNow } from '@/features/library/useNow';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { duration, ease, spring, useMotionReduced } from '@/shared/motion/motion';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { HoldButton } from '@/shared/ui/HoldButton';
import { btnQuiet, chip } from '@/shared/ui/classes';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { Card } from './controls';
import { useRecentlyDeleted } from './queries';

const DAY_MS = 86_400_000;

/** Whole days until `expiresAt`, counting a part of a day as one. */
export function daysLeft(expiresAt: string, now: number): number {
  return Math.max(1, Math.ceil((Date.parse(expiresAt) - now) / DAY_MS));
}

type Leaving = 'restore' | 'purge';

const refresh = (): void => {
  void queryClient.invalidateQueries({ queryKey: ['recently-deleted'] });
  void queryClient.invalidateQueries({ queryKey: ['games'] });
  void queryClient.invalidateQueries({ queryKey: ['game'] });
  void queryClient.invalidateQueries({ queryKey: ['sync-report'] });
};

export function RecentlyDeletedSection(): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const { data: items = [] } = useRecentlyDeleted();
  /** How each row on its way out leaves, so its exit heads the right way. */
  const [leaving, setLeaving] = useState<Record<string, Leaving>>({});
  const [purging, setPurging] = useState<DeletedItem | null>(null);
  const now = useNow(60_000).getTime();

  /** A row that failed to leave stays, usable again. */
  const stay = (uuid: string): void => {
    setLeaving(({ [uuid]: _, ...others }) => others);
  };

  async function restore(item: DeletedItem): Promise<void> {
    setLeaving((l) => ({ ...l, [item.uuid]: 'restore' }));
    try {
      await (item.kind === 'game' ? recoverGame(item.uuid) : recoverSlot(item.uuid));
      toast.success(item.kind === 'game' ? t.settings.restored(item.title) : t.settings.restoredSlot(item.title));
    } catch (err) {
      stay(item.uuid);
      reportError(t.settings.failed.restore, err, addFailedHint(err, t.addGames.why));
    }
    refresh();
  }

  async function purge(item: DeletedItem): Promise<void> {
    setPurging(null);
    setLeaving((l) => ({ ...l, [item.uuid]: 'purge' }));
    try {
      await purgeDeleted(item.uuid);
    } catch (err) {
      stay(item.uuid);
      reportError(t.settings.failed.deleteNow, err);
    }
    refresh();
  }

  async function exportHistory(item: DeletedItem): Promise<void> {
    try {
      const destination = await pickSavePath(t.game.exportTo, t.game.exportName(item.title), [{ name: t.game.bundleFiles, extensions: ['zip'] }]);
      if (destination === null) return;
      const report = await exportDeletedGame(item.uuid, destination);
      toast.success(t.game.exported(report.version_count, report.title));
    } catch (err) {
      reportError(t.game.failed.export, err);
    }
  }

  const iconButton = 'flex size-[30px] shrink-0 items-center justify-center rounded-[9px] text-tx3 transition-[background-color,color,transform] duration-150 ease-spring hover:bg-ln hover:text-tx active:scale-[0.88]';

  return (
    <>
      <AnimatePresence initial={false}>
        {items.length > 0 && (
          <motion.div
            key="recently-deleted"
            initial={reduced ? false : { opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduced ? undefined : { opacity: 0, y: 12 }}
            transition={reduced ? { duration: 0 } : spring}
          >
            <Card title={t.settings.recentlyDeleted}>
              <p className="mt-1.5 text-[12.5px] text-tx3">{t.settings.recentlyDeletedHint}</p>
              <ul className="mt-3 flex flex-col gap-2">
                <AnimatePresence initial={false}>
                  {items.map((item, i) => (
                    <motion.li
                      key={item.uuid}
                      layout={!reduced}
                      className="flex items-center gap-2 rounded-[14px] bg-el py-2 pl-3.5 pr-2"
                      initial={reduced ? false : { opacity: 0, y: 8 }}
                      animate={{ opacity: 1, x: 0, y: 0, scale: 1 }}
                      // Restored rows head back toward the library on the left; deleted ones sink away.
                      exit={reduced ? { opacity: 0, transition: { duration: 0 } }
                        : leaving[item.uuid] === 'restore' ? { opacity: 0, x: -40, transition: { duration: duration.fast, ease } }
                        : { opacity: 0, y: 6, scale: 0.96, transition: { duration: duration.fast, ease } }}
                      transition={reduced ? { duration: 0 } : { ...spring, delay: Math.min(i, 5) * 0.04 }}
                    >
                      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span className="truncate text-sm font-semibold">{item.title}</span>
                        <span className="truncate text-xs text-tx3">
                          {item.kind === 'slot' ? `${t.settings.playthroughOf(item.game_title)} · ` : ''}
                          {t.settings.daysLeft(daysLeft(item.expires_at, now))}
                        </span>
                      </span>
                      <button
                        type="button"
                        aria-label={t.settings.restoreTitle(item.title)}
                        className={cn(chip, 'bg-ln text-tx')}
                        disabled={item.uuid in leaving}
                        onClick={() => { void restore(item); }}
                      >
                        {t.settings.restore}
                      </button>
                      {item.kind === 'game' && (
                        <Tooltip content={t.settings.exportDeleted(item.title)}>
                          <button type="button" aria-label={t.settings.exportDeleted(item.title)} className={iconButton} onClick={() => { void exportHistory(item); }}>
                            <Download size={15} strokeWidth={2.2} aria-hidden="true" />
                          </button>
                        </Tooltip>
                      )}
                      <Tooltip content={t.settings.deleteNow(item.title)}>
                        <button
                          type="button"
                          aria-label={t.settings.deleteNow(item.title)}
                          className={cn(iconButton, 'hover:text-err')}
                          disabled={item.uuid in leaving}
                          onClick={() => { setPurging(item); }}
                        >
                          <Trash2 size={15} strokeWidth={2.2} aria-hidden="true" />
                        </button>
                      </Tooltip>
                    </motion.li>
                  ))}
                </AnimatePresence>
              </ul>
            </Card>
          </motion.div>
        )}
      </AnimatePresence>

      <Dialog open={purging !== null} onOpenChange={(o) => { if (!o) setPurging(null); }}>
        <DialogContent className="bg-sf sm:max-w-[420px]">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.settings.deleteNowTitle(purging?.title ?? '')}</DialogTitle>
            <DialogDescription className="text-tx2">{t.settings.deleteNowBody}</DialogDescription>
          </DialogHeader>
          <DialogFooter className="items-center gap-2">
            <button type="button" className={btnQuiet} onClick={() => { setPurging(null); }}>{t.settings.cancel}</button>
            <HoldButton
              label={t.settings.holdToDelete}
              hint={t.settings.holdHint}
              disabled={false}
              danger
              icon={<Trash2 size={15} strokeWidth={2.4} aria-hidden="true" />}
              className="mt-0 h-[38px] flex-1 rounded-xl text-sm"
              onComplete={() => { if (purging !== null) void purge(purging); }}
            />
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

// Installed games on this PC: scan on request, pick from what turns up, add them all at once.
// Also where the folders the scan looks in are managed.
import { type ReactElement, useMemo, useRef, useState } from 'react';
import { motion } from 'motion/react';
import { Check, Plus, X } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { addScanRoot, pickFolder, removeScanRoot, saveGame, setGameIcon } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { GameIcon } from '@/features/library/GameIcon';
import { useShellNav } from '@/features/library/shell-nav';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { btn, btnQuiet, chip, eyebrow } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';
import { newScanResults } from './add-games';
import { useInstalledScan, useScanRoots } from './queries';

export function ScanSection({ library, cloudTitles }: { library: FullGame[]; cloudTitles: string[] }): ReactElement {
  const t = useT();
  const [started, setStarted] = useState(false);
  const scan = useInstalledScan(started);

  return (
    <section aria-labelledby="add-scan">
      <h2 id="add-scan" className={eyebrow}>{t.addGames.onThisPc}</h2>
      <div className="mt-3">
        {!started ? (
          <div className="flex h-[176px] max-w-[664px] flex-col items-center justify-center gap-3 rounded-[18px] border-[1.5px] border-dashed border-ln">
            <button type="button" className={btn} onClick={() => { setStarted(true); }}>{t.addGames.lookForGames}</button>
            <span className="text-[13px] text-tx3">{t.addGames.scanSources}</span>
          </div>
        ) : scan.isFetching ? (
          <div role="status" className="relative flex h-[176px] max-w-[664px] items-center justify-center overflow-hidden rounded-[18px] border-[1.5px] border-dashed border-acc-line">
            <span aria-hidden="true" className="absolute inset-y-0 left-0 w-[30%] bg-[linear-gradient(90deg,transparent,var(--acc-soft),transparent)] [animation:sweep_1.1s_var(--ez)_infinite]" />
            <span className="relative text-sm font-semibold text-tx2">{t.addGames.scanning}</span>
          </div>
        ) : scan.error !== null ? (
          <div className="flex h-[176px] max-w-[664px] flex-col items-center justify-center gap-3 rounded-[18px] bg-sf">
            <span className="text-sm text-tx2">{t.addGames.failed.scan}</span>
            <button type="button" className={btnQuiet} onClick={() => { void scan.refetch(); }}>{t.errors.retry}</button>
          </div>
        ) : (
          <Results
            // A new scan starts a new selection.
            key={scan.dataUpdatedAt}
            found={newScanResults(scan.data ?? [], library, cloudTitles)}
            onLookAgain={() => { void scan.refetch(); }}
          />
        )}
      </div>
      <ScanRoots />
    </section>
  );
}

function Results({ found, onLookAgain }: { found: InstallScannerItem[]; onLookAgain: () => void }): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const { land } = useShellNav();
  // The list is fixed at the scan; added games stay in it, faded.
  const [items] = useState(found);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(found.map((f) => f.title)));
  const [added, setAdded] = useState<ReadonlySet<string>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const iconsRef = useRef(new Map<string, HTMLElement>());
  const chosen = useMemo(() => items.filter((i) => selected.has(i.title) && !added.has(i.title)), [items, selected, added]);

  function toggle(title: string): void {
    setSelected((all) => {
      const next = new Set(all);
      if (!next.delete(title)) next.add(title);
      return next;
    });
  }

  async function addChosen(): Promise<void> {
    setBusy(true);
    for (const item of chosen) {
      try {
        const game = await saveGame({ id: null, title: item.title }, item.dirs);
        if (item.icon_path !== null) {
          // The game is added either way; it just keeps its initials.
          game.icon_path = await setGameIcon(game.id, item.icon_path).catch(() => null);
        }
        const from = iconsRef.current.get(item.title)?.getBoundingClientRect();
        setAdded((all) => new Set(all).add(item.title));
        await queryClient.invalidateQueries({ queryKey: ['games'] });
        if (from !== undefined) land(game, from);
      } catch (err) {
        reportError(t.addGames.failed.add, err);
      }
    }
    setBusy(false);
  }

  if (items.length === 0) {
    return (
      <div className="flex h-[176px] max-w-[664px] flex-col items-center justify-center gap-3 rounded-[18px] bg-sf">
        <span className="text-sm text-tx2">{t.addGames.nothingFound}</span>
        <button type="button" className={btnQuiet} onClick={onLookAgain}>{t.addGames.lookAgain}</button>
      </div>
    );
  }

  return (
    <>
      <ul className="flex flex-wrap gap-4">
        {items.map((item, i) => {
          const done = added.has(item.title);
          const on = selected.has(item.title) && !done;
          return (
            <motion.li
              key={item.title}
              initial={reduced ? false : { opacity: 0, y: 10, scale: 0.97 }}
              animate={{ opacity: done ? 0.35 : 1, y: 0, scale: 1 }}
              transition={{ duration: 0.42, ease, delay: reduced ? 0 : i * 0.09 }}
            >
              <button
                type="button"
                aria-pressed={on}
                disabled={done || busy}
                onClick={() => { toggle(item.title); }}
                className={cn(
                  'relative flex h-[176px] w-[210px] flex-col justify-end gap-1 rounded-[18px] bg-sf p-[18px] text-left transition-[box-shadow,transform] duration-200 ease-spring hover:-translate-y-0.5 active:scale-[0.97] disabled:pointer-events-none',
                  on && 'shadow-[inset_0_0_0_2px_var(--acc)]',
                )}
              >
                <span
                  className="absolute left-[18px] top-[18px]"
                  ref={(el) => { if (el !== null) iconsRef.current.set(item.title, el); }}
                >
                  <GameIcon title={item.title} iconPath={item.icon_path ?? undefined} size={56} />
                </span>
                <span
                  aria-hidden="true"
                  className={cn(
                    'absolute right-4 top-4 flex size-6 items-center justify-center rounded-lg text-acc-ink transition-colors duration-200',
                    on ? 'bg-acc' : 'shadow-[inset_0_0_0_1.5px_var(--tx3)]',
                  )}
                >
                  {on && <Check size={14} strokeWidth={3} />}
                </span>
                <span className="truncate text-[15px] font-bold">{item.title}</span>
                <span className="text-[12.5px] text-tx3">{t.addGames.source[item.source]}</span>
              </button>
            </motion.li>
          );
        })}
      </ul>
      <div className="mt-4 flex items-center gap-3">
        <button type="button" className={btn} disabled={chosen.length === 0 || busy} onClick={() => { void addChosen(); }}>
          {chosen.length > 0 || added.size === 0 ? t.addGames.addFound(chosen.length) : t.addGames.allAdded}
        </button>
        <button type="button" className={btnQuiet} disabled={busy} onClick={onLookAgain}>{t.addGames.lookAgain}</button>
      </div>
    </>
  );
}

function ScanRoots(): ReactElement {
  const t = useT();
  const { data: roots = [] } = useScanRoots();

  async function change(action: () => Promise<void>): Promise<void> {
    try {
      await action();
      await queryClient.invalidateQueries({ queryKey: ['scan-roots'] });
    } catch (err) {
      reportError(t.addGames.failed.scanFolders, err);
    }
  }

  async function add(): Promise<void> {
    const path = await pickFolder(t.addGames.pickScanFolder);
    if (path !== null) await change(() => addScanRoot(path));
  }

  return (
    <div className="mt-4 flex flex-wrap items-center gap-2">
      <span className="mr-1 text-[13px] text-tx3">{t.addGames.yourFolders}</span>
      {roots.map((root) => (
        <span key={root} className={cn(chip, 'max-w-[320px] pr-1.5 hover:bg-el active:scale-100')}>
          <span className="truncate font-mono text-xs" title={root}>{root}</span>
          <button
            type="button"
            aria-label={t.addGames.stopLooking(root)}
            className="flex size-5 shrink-0 items-center justify-center rounded-full text-tx3 transition-colors hover:bg-ln hover:text-tx"
            onClick={() => { void change(() => removeScanRoot(root)); }}
          >
            <X size={12} strokeWidth={2.6} aria-hidden="true" />
          </button>
        </span>
      ))}
      <button type="button" className={chip} onClick={() => { void add(); }}>
        <Plus size={14} strokeWidth={2.4} aria-hidden="true" />
        {t.addGames.addScanFolder}
      </button>
    </div>
  );
}

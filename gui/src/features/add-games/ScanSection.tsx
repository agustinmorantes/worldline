// Installed games on this PC: scan on request, pick from what turns up, add them all at once.
// Also where the folders the scan looks in are managed.
import { type ReactElement, type RefObject, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Check, LayoutGrid, List, LoaderCircle, Minus, Plus, Search, X } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { addScanRoot, pickFolder, removeScanRoot } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { GameIcon } from '@/features/library/GameIcon';
import { matchesTitle } from '@/features/library/library';
import { useShellNav } from '@/features/library/shell-nav';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { ease, spring, springEase, useMotionReduced } from '@/shared/motion/motion';
import { setPref, usePrefs } from '@/shared/prefs/prefs';
import { btn, btnQuiet, chip, eyebrow, field } from '@/shared/ui/classes';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { newScanResults, scanRevealDelay } from './add-games';
import { addAll, onGameAdded, useAddRun } from './add-run';
import { useInstalledScan, useScanRoots } from './queries';

const noneAdded: ReadonlySet<string> = new Set();

export function ScanSection({ library, cloudTitles, autoScan = false }: {
  library: FullGame[];
  cloudTitles: string[];
  /** Skip the "Scan this PC" prompt. */
  autoScan?: boolean;
}): ReactElement {
  const t = useT();
  const [asked, setAsked] = useState(false);
  const started = asked || autoScan;
  const scan = useInstalledScan(started);

  return (
    <section aria-labelledby="add-scan">
      <h2 id="add-scan" className={eyebrow}>{t.addGames.onThisPc}</h2>
      <div className="mt-3">
        {!started ? (
          <div className="flex h-[176px] flex-col items-center justify-center gap-3 rounded-[18px] border-[1.5px] border-dashed border-ln">
            <button type="button" className={btn} onClick={() => { setAsked(true); }}>{t.addGames.lookForGames}</button>
            <span className="text-[13px] text-tx3">{t.addGames.scanSources}</span>
          </div>
        ) : scan.isFetching ? (
          <div role="status" className="relative flex h-[176px] items-center justify-center overflow-hidden rounded-[18px] border-[1.5px] border-dashed border-acc-line">
            <span aria-hidden="true" className="absolute inset-y-0 left-0 w-[30%] bg-[linear-gradient(90deg,transparent,var(--acc-soft),transparent)] [animation:sweep_1.1s_var(--ez)_infinite]" />
            <span className="relative text-sm font-semibold text-tx2">{t.addGames.scanning}</span>
          </div>
        ) : scan.error !== null ? (
          <div className="flex h-[176px] flex-col items-center justify-center gap-3 rounded-[18px] bg-sf">
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
  // The run outlives this view: coming back mid-run shows it, and can't start another.
  const run = useAddRun();
  const added = run?.added ?? noneAdded;
  const busy = run?.running ?? false;
  const progress = run === null ? 0 : run.added.size + run.failed.size;
  const [query, setQuery] = useState('');
  const [ripple, setRipple] = useState<ReadonlyMap<string, number>>(() => new Map());
  const { scanView } = usePrefs();
  const list = scanView === 'list';
  const revealedRef = useRef(false);
  useEffect(() => { revealedRef.current = true; }, []);
  const [topSentinel, topFloating] = useStickyEdge('top');
  const [bottomSentinel, bottomFloating] = useStickyEdge('bottom');
  const allRef = useRef<HTMLInputElement>(null);
  const iconsRef = useRef(new Map<string, HTMLElement>());
  const chosen = useMemo(() => items.filter((i) => selected.has(i.title) && !added.has(i.title)), [items, selected, added]);
  const visible = items.filter((item) => matchesTitle(item.title, query));
  const available = visible.filter((item) => !added.has(item.title));
  const checked = available.filter((item) => selected.has(item.title)).length;
  const allChecked = available.length > 0 && checked === available.length;
  const mixed = checked > 0 && !allChecked;
  useEffect(() => { if (allRef.current !== null) allRef.current.indeterminate = mixed; }, [mixed]);

  function toggle(title: string): void {
    setRipple(new Map());
    setSelected((all) => {
      const next = new Set(all);
      if (!next.delete(title)) next.add(title);
      return next;
    });
  }

  function toggleAll(): void {
    setRipple(new Map(available.map((item, i) => [item.title, i * 0.25 / Math.max(1, available.length - 1)])));
    setSelected((all) => {
      const next = new Set(all);
      for (const item of available) {
        if (allChecked) next.delete(item.title);
        else next.add(item.title);
      }
      return next;
    });
  }

  // Each added game flies from its card into the sidebar, while this view is open.
  useEffect(() => onGameAdded((title, game) => {
    const from = iconsRef.current.get(title)?.getBoundingClientRect();
    if (from !== undefined) land(game, from);
  }), [land]);

  if (items.length === 0) {
    return (
      <div className="flex h-[176px] flex-col items-center justify-center gap-3 rounded-[18px] bg-sf">
        <span className="text-sm text-tx2">{t.addGames.nothingFound}</span>
        <button type="button" className={btnQuiet} onClick={onLookAgain}>{t.addGames.lookAgain}</button>
      </div>
    );
  }

  return (
    <div className="relative">
      <div ref={topSentinel} className="h-px" aria-hidden="true" />
      <div className="sticky top-0 z-[2] mb-4 flex flex-wrap items-center gap-3 py-2">
        <StickyBackdrop floating={topFloating} edge="top" />
        <div className="relative min-w-[180px] flex-1">
          <Search size={16} aria-hidden="true" className="pointer-events-none absolute left-3 top-3 text-tx3" />
          <input type="search" aria-label={t.addGames.searchPlaceholder} placeholder={t.addGames.searchPlaceholder} value={query}
            onChange={(event) => { setQuery(event.target.value); }} className={cn(field, 'pl-9')} />
        </div>
        {/* A native checkbox (keyboard, indeterminate) drawn like the cards' check boxes. */}
        <label className="relative flex h-10 cursor-pointer items-center gap-2.5 rounded-xl bg-el pl-3 pr-3.5 text-[13px] font-semibold text-tx2 transition-colors duration-200 hover:text-tx has-[:disabled]:pointer-events-none has-[:disabled]:opacity-40 has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-acc">
          <input ref={allRef} type="checkbox" checked={allChecked} aria-checked={mixed ? 'mixed' : allChecked}
            disabled={busy || available.length === 0} onChange={toggleAll} className="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-xl opacity-0" />
          <span
            aria-hidden="true"
            className={cn(
              'flex size-[18px] items-center justify-center rounded-[6px] text-acc-ink transition-[background-color,box-shadow] duration-200',
              allChecked || mixed ? 'bg-acc' : 'shadow-[inset_0_0_0_1.5px_var(--tx3)]',
            )}
          >
            {allChecked ? <Check size={12} strokeWidth={3.5} /> : mixed && <Minus size={12} strokeWidth={3.5} />}
          </span>
          {t.addGames.selectAll}
        </label>
        <div className="flex rounded-xl bg-el p-1">
          {(['grid', 'list'] as const).map((view) => (
            <button key={view} type="button" aria-label={view === 'grid' ? t.addGames.gridView : t.addGames.listView}
              aria-pressed={scanView === view} onClick={() => { setPref('scanView', view); }}
              className="relative flex size-8 items-center justify-center rounded-lg text-tx2">
              {scanView === view && <motion.span layoutId="scan-view-thumb" className="absolute inset-0 rounded-lg bg-ln" transition={reduced ? { duration: 0 } : spring} />}
              {view === 'grid' ? <LayoutGrid size={16} className="relative" /> : <List size={16} className="relative" />}
            </button>
          ))}
        </div>
      </div>
      <ul className={cn('relative grid gap-4', list ? 'grid-cols-1 gap-2' : 'grid-cols-[repeat(auto-fill,minmax(210px,1fr))]')}>
        <AnimatePresence mode="popLayout">
        {visible.map((item) => {
          const i = items.indexOf(item);
          const done = added.has(item.title);
          const on = selected.has(item.title) && !done;
          const adding = run?.current === item.title;
          return (
            <motion.li
              key={item.title}
              layout
              layoutId={`scan-item-${item.title}`}
              className="min-w-0"
              initial={reduced || revealedRef.current ? false : { opacity: 0, y: 10, scale: 0.97 }}
              animate={{ opacity: done ? 0.35 : 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95, transition: { duration: reduced ? 0 : 0.2 } }}
              transition={{ duration: reduced ? 0 : 0.42, ease, delay: reduced || revealedRef.current ? 0 : scanRevealDelay(i), layout: reduced ? { duration: 0 } : spring }}
            >
              <button
                type="button"
                aria-pressed={on}
                disabled={done || busy}
                onClick={() => { toggle(item.title); }}
                className={cn(
                  'relative flex w-full rounded-[18px] bg-sf text-left transition-[box-shadow,transform] duration-200 ease-spring hover:-translate-y-0.5 active:scale-[0.97] disabled:pointer-events-none',
                  list ? 'h-[52px] items-center gap-3 px-3' : 'h-[176px] flex-col justify-end gap-1 p-[18px]',
                  on && 'shadow-[inset_0_0_0_2px_var(--acc)]',
                )}
              >
                <motion.span
                  layout layoutId={`scan-icon-${item.title}`}
                  transition={reduced ? { duration: 0 } : spring}
                  className={list ? 'shrink-0' : 'absolute left-[18px] top-[18px]'}
                  ref={(el) => { if (el !== null) iconsRef.current.set(item.title, el); else iconsRef.current.delete(item.title); }}
                >
                  <GameIcon title={item.title} iconPath={item.icon_path ?? undefined} size={list ? 32 : 56} />
                </motion.span>
                <motion.span
                  aria-hidden="true"
                  className={cn(
                    'absolute right-4 flex size-6 items-center justify-center rounded-lg text-acc-ink shadow-[inset_0_0_0_1.5px_var(--tx3)]',
                    list ? 'top-[14px]' : 'top-4',
                  )}
                  animate={{ backgroundColor: on ? 'var(--acc)' : 'transparent', scale: on ? [1, 1.15, 1] : 1 }}
                  transition={{ duration: reduced ? 0 : 0.24, ease: springEase, delay: reduced ? 0 : (ripple.get(item.title) ?? 0) }}
                >
                  <motion.span animate={{ opacity: on && !adding ? 1 : 0 }} transition={{ duration: reduced ? 0 : 0.16, delay: reduced ? 0 : (ripple.get(item.title) ?? 0) }}><Check size={14} strokeWidth={3} /></motion.span>
                  {adding && <LoaderCircle size={14} strokeWidth={3} aria-hidden="true" className={cn('absolute', !reduced && 'animate-spin')} />}
                </motion.span>
                <span className={cn('truncate text-[15px] font-bold', list && 'min-w-0 flex-1')}>{item.title}</span>
                <span className={cn('text-[12.5px] text-tx3', list && 'mr-10 shrink-0')}>{t.addGames.source[item.source]}</span>
              </button>
            </motion.li>
          );
        })}
        </AnimatePresence>
      </ul>
      <AnimatePresence>
        {visible.length === 0 && <motion.p role="status" className="py-6 text-sm text-tx3"
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}
          transition={{ duration: reduced ? 0 : 0.2 }}>{t.addGames.noMatches}</motion.p>}
      </AnimatePresence>
      <motion.div layout="position" transition={reduced ? { duration: 0 } : spring} className="sticky bottom-0 z-[2] mt-4 flex items-center gap-3 py-2">
        <StickyBackdrop floating={bottomFloating} edge="bottom" />
        {run?.running === true ? (
          <div role="status" className="flex min-w-0 flex-1 flex-col gap-1.5">
            <span className="text-sm font-semibold">{t.addGames.adding(progress, run.total)}</span>
            <div className="h-1.5 overflow-hidden rounded-full bg-el" aria-hidden="true">
              <motion.div className="h-full rounded-full bg-acc" initial={false}
                animate={{ width: `${(100 * progress / run.total).toString()}%` }} transition={{ duration: reduced ? 0 : 0.3, ease }} />
            </div>
            {run.current !== null && <span className="truncate text-[12.5px] text-tx3">{t.addGames.addingNow(run.current)}</span>}
          </div>
        ) : (
          <>
            <button type="button" className={btn} disabled={chosen.length === 0} onClick={() => { void addAll(chosen); }}>
              {chosen.length > 0 ? t.addGames.addFound(chosen.length) : added.size === 0 ? t.addGames.title : t.addGames.allAdded}
            </button>
            <button type="button" className={btnQuiet} onClick={onLookAgain}>{t.addGames.lookAgain}</button>
          </>
        )}
      </motion.div>
      <div ref={bottomSentinel} className="h-px" aria-hidden="true" />
    </div>
  );
}

/** Observe the natural position, so a bar lifts only while pinned to the scrollport. */
function useStickyEdge(edge: 'top' | 'bottom'): [RefObject<HTMLDivElement | null>, boolean] {
  const ref = useRef<HTMLDivElement>(null);
  const [floating, setFloating] = useState(false);
  useEffect(() => {
    const sentinel = ref.current;
    if (sentinel === null) return;
    const root = sentinel.closest('[data-add-scroll]');
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.rootBounds === null) return;
      setFloating(edge === 'top' ? entry.boundingClientRect.top < entry.rootBounds.top : entry.boundingClientRect.bottom > entry.rootBounds.bottom);
    }, { root, threshold: [0, 1] });
    observer.observe(sentinel);
    return () => { observer.disconnect(); };
  }, [edge]);
  return [ref, floating];
}

function StickyBackdrop({ floating, edge }: { floating: boolean; edge: 'top' | 'bottom' }): ReactElement {
  const reduced = useMotionReduced();
  return <motion.span aria-hidden="true" initial={false} animate={{ opacity: floating ? 1 : 0 }}
    transition={{ duration: reduced ? 0 : 0.24, ease }}
    className={cn('pointer-events-none absolute -inset-x-2 inset-y-0 -z-10 rounded-xl bg-[color-mix(in_oklch,var(--bg)_85%,transparent)] backdrop-blur-md',
      edge === 'bottom' ? 'shadow-[0_-12px_24px_oklch(0%_0_0/0.15)]' : 'shadow-[0_12px_24px_oklch(0%_0_0/0.15)]')} />;
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
          <Tooltip mono content={root}>
            <span className="truncate font-mono text-xs">{root}</span>
          </Tooltip>
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

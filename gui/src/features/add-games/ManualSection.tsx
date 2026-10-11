// For games the scan can't find: add one from its save folder, or import a history bundle
// exported from another install.
import { type ReactElement, type ReactNode, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { FolderOpen, Package } from 'lucide-react';
import { toast } from 'sonner';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { HistoryBundleSummary } from '@/ipc/bindings/HistoryBundleSummary';
import { getGames, importGameHistory, inspectHistoryBundle, pickFile, pickFolder, saveGame } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { GameIcon } from '@/features/library/GameIcon';
import { useShellNav } from '@/features/library/shell-nav';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { btn, btnQuiet, eyebrow, field } from '@/shared/ui/classes';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { addFailedHint, dirsToMap, folderName, isTooLarge } from './add-games';

type Draft =
  | { kind: 'folder'; path: string; title: string }
  | { kind: 'bundle'; path: string; summary: HistoryBundleSummary; mappings: Record<string, string> };

export function ManualSection({ library }: { library: FullGame[] }): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const { go } = useShellNav();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);

  async function pickSaveFolder(): Promise<void> {
    const path = await pickFolder(t.game.pickSaveFolder);
    if (path !== null) setDraft({ kind: 'folder', path, title: folderName(path) });
  }

  async function openBundle(): Promise<void> {
    const path = await pickFile(t.addGames.pickBundle, [{ name: t.game.bundleFiles, extensions: ['zip'] }]);
    if (path === null) return;
    try {
      setDraft({ kind: 'bundle', path, summary: await inspectHistoryBundle(path), mappings: {} });
    } catch (err) {
      reportError(t.addGames.failed.bundle, err);
    }
  }

  /**
   * Runs `action`, then opens the game it returns once the library has it. `insist`, when given,
   * is the same add for saves the user confirms are that large.
   */
  async function addThen(action: () => Promise<number | undefined>, failed: string, insist?: () => void): Promise<void> {
    setBusy(true);
    try {
      const id = await action();
      await queryClient.invalidateQueries({ queryKey: ['games'] });
      setDraft(null);
      if (id !== undefined) go({ view: 'game', gameId: id });
    } catch (err) {
      const anyway = insist !== undefined && isTooLarge(err) ? { label: t.addGames.trackAnyway, onClick: insist } : undefined;
      reportError(failed, err, addFailedHint(err, t.addGames.why), anyway);
    } finally {
      setBusy(false);
    }
  }

  function addFromFolder(d: Extract<Draft, { kind: 'folder' }>, allowLarge = false): void {
    void addThen(
      async () => (await saveGame({ id: null, title: d.title.trim() }, [d.path], allowLarge)).id,
      t.addGames.failed.add,
      allowLarge ? undefined : () => { addFromFolder(d, true); },
    );
  }

  function importBundle(d: Extract<Draft, { kind: 'bundle' }>): void {
    void addThen(async () => {
      const report = await importGameHistory(d.path, d.mappings);
      await queryClient.invalidateQueries({ queryKey: ['game'] });
      await queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
      toast.success(t.addGames.imported(report.version_count, report.title));
      return (await getGames()).find((g) => g.uuid === report.game_uuid)?.id;
    }, t.addGames.failed.import);
  }

  return (
    <section aria-labelledby="add-else">
      <h2 id="add-else" className={eyebrow}>{t.addGames.somethingElse}</h2>
      <div className="mt-3 grid grid-cols-2 gap-4">
        <BigButton icon={<FolderOpen size={22} strokeWidth={1.8} />} title={t.addGames.pickSaveFolder} sub={t.addGames.pickSaveFolderSub} disabled={busy} onClick={() => { void pickSaveFolder(); }} />
        <BigButton icon={<Package size={22} strokeWidth={1.8} />} title={t.addGames.openBundle} sub={t.addGames.openBundleSub} disabled={busy} onClick={() => { void openBundle(); }} />
      </div>
      <AnimatePresence mode="wait">
        {draft !== null && (
          <motion.div
            key={draft.path}
            initial={reduced ? false : { opacity: 0, y: 10, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={reduced ? undefined : { opacity: 0, y: 6, transition: { duration: 0.18 } }}
            transition={{ duration: 0.36, ease }}
            className="mt-4 max-w-[664px] rounded-[18px] bg-sf p-5"
          >
            {draft.kind === 'folder' ? (
              <form
                className="flex flex-col gap-3"
                onSubmit={(e) => { e.preventDefault(); addFromFolder(draft); }}
              >
                <label className="flex flex-col gap-1.5">
                  <span className="text-[13px] font-semibold text-tx2">{t.addGames.gameName}</span>
                  <input
                    className={field}
                    value={draft.title}
                    autoFocus
                    onChange={(e) => { setDraft({ ...draft, title: e.target.value }); }}
                  />
                </label>
                <Tooltip mono content={draft.path}>
                  <span className="truncate font-mono text-xs text-tx3">{draft.path}</span>
                </Tooltip>
                <Actions
                  confirm={t.addGames.addGame}
                  disabled={busy || draft.title.trim() === ''}
                  onCancel={() => { setDraft(null); }}
                />
              </form>
            ) : (
              <BundleForm
                draft={draft}
                library={library}
                busy={busy}
                onChange={setDraft}
                onCancel={() => { setDraft(null); }}
                onImport={() => { importBundle(draft); }}
              />
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}

function BundleForm({ draft, library, busy, onChange, onCancel, onImport }: {
  draft: Extract<Draft, { kind: 'bundle' }>;
  library: FullGame[];
  busy: boolean;
  onChange: (draft: Draft) => void;
  onCancel: () => void;
  onImport: () => void;
}): ReactElement {
  const t = useT();
  const { summary, mappings } = draft;
  const toMap = dirsToMap(summary, library);
  const ready = toMap.every((dir) => (mappings[dir.uuid] ?? '') !== '');

  async function choose(uuid: string): Promise<void> {
    const path = await pickFolder(t.game.pickSaveFolder);
    if (path !== null) onChange({ ...draft, mappings: { ...mappings, [uuid]: path } });
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); onImport(); }}>
      <div className="flex items-center gap-3.5">
        <GameIcon title={summary.title} size={48} />
        <span className="flex min-w-0 flex-col gap-[3px]">
          <span className="truncate text-[15px] font-bold">{summary.title}</span>
          <span className="text-[12.5px] text-tx2">{t.addGames.bundleMeta(summary.version_count, summary.slot_count)}</span>
        </span>
      </div>
      {toMap.length > 0 && (
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-2 text-[13px] font-semibold text-tx2">{t.addGames.whereSaves}</legend>
          {toMap.map((dir) => {
            const chosen = mappings[dir.uuid] as string | undefined;
            const name = dir.label ?? t.addGames.saveFolder;
            return (
              <div key={dir.uuid} className="flex items-center gap-3 rounded-xl bg-el px-3 py-2.5">
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <Tooltip mono content={chosen}>
                    <span className={cn('truncate text-sm font-semibold', chosen === undefined ? 'text-tx2' : 'font-mono text-xs text-tx')}>
                      {chosen ?? name}
                    </span>
                  </Tooltip>
                  <Tooltip mono content={dir.source_path_hint}>
                    <span className="truncate text-xs text-tx3">{t.addGames.wasAt(dir.source_path_hint)}</span>
                  </Tooltip>
                </span>
                <button
                  type="button"
                  className={btnQuiet}
                  aria-label={`${t.addGames.chooseFolder}: ${name}`}
                  onClick={() => { void choose(dir.uuid); }}
                >
                  {t.addGames.chooseFolder}
                </button>
              </div>
            );
          })}
        </fieldset>
      )}
      <Actions confirm={t.addGames.importBundle} disabled={busy || !ready} onCancel={onCancel} />
    </form>
  );
}

function Actions({ confirm, disabled, onCancel }: { confirm: string; disabled: boolean; onCancel: () => void }): ReactElement {
  const t = useT();
  return (
    <div className="flex gap-2.5">
      <button type="submit" className={btn} disabled={disabled}>{confirm}</button>
      <button type="button" className={btnQuiet} onClick={onCancel}>{t.addGames.cancel}</button>
    </div>
  );
}

function BigButton({ icon, title, sub, disabled, onClick }: {
  icon: ReactNode;
  title: string;
  sub: string;
  disabled: boolean;
  onClick: () => void;
}): ReactElement {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="flex h-[72px] w-full items-center gap-3.5 rounded-[18px] bg-sf px-[18px] text-left text-tx2 transition-[background-color,transform] duration-200 ease-spring hover:bg-el active:scale-[0.97] disabled:opacity-60"
    >
      <span aria-hidden="true">{icon}</span>
      <span className="flex flex-col gap-[3px]">
        <span className="text-sm font-semibold text-tx">{title}</span>
        <span className="text-[12.5px] font-medium text-tx3">{sub}</span>
      </span>
    </button>
  );
}

import { createFileRoute } from '@tanstack/react-router';
import { type ReactElement, useMemo, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { useGame, useRetiredVersions, useRunningGames, useSyncReport, useSyncSettings } from '@/features/library/queries';
import { useSyncRunning } from '@/features/library/sync-activity';
import { useNow } from '@/features/library/useNow';
import { GameHeader, type GameView } from '@/features/game/GameHeader';
import { checkpointDevice, pendingRemoteVersion, syncChip } from '@/features/game/game-view';
import { CheckpointPanel } from '@/features/sessions/CheckpointPanel';
import { nameCheckpoint } from '@/features/sessions/name-checkpoint';
import { SessionList } from '@/features/sessions/SessionList';
import { EmptyGame, formatDuration, GhostRow, TimelineFrame } from '@/features/sessions/Timeline';
import { GraphView } from '@/features/graph/GraphView';
import { graphNavOrder, layoutGraph } from '@/features/graph/graph';
import { useDevice, useMonitoringSettings, useUnrecorded } from '@/features/sessions/queries';
import { layoutSessions, navOrder, playedMs, sessionGapMs, splitSessions, totalPlayedMs } from '@/features/sessions/sessions';
import type { Version } from '@/ipc/bindings/Version';
import { applyRemoteVersion, createSlot, restoreVersion, switchSlot } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import type { Failure } from '@/ipc/bindings/Failure';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { btn } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';

interface GameSearch {
  /** The selected checkpoint; your current one when absent. */
  v?: number;
  view?: GameView;
}

export const Route = createFileRoute('/game/$gameId')({
  validateSearch: (search: Record<string, unknown>): GameSearch => ({
    v: typeof search.v === 'number' ? search.v : undefined,
    view: search.view === 'graph' ? 'graph' : undefined,
  }),
  component: GameDetail,
});

/** How long the rewind streaks play before the list settles. */
const REWIND_MS = 640;

export function GameDetail(): ReactElement {
  const t = useT();
  const { gameId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const id = parseInt(gameId);
  const { data: game, isLoading, error, refetch } = useGame(id);
  const { data: settings } = useSyncSettings();
  const report = useSyncReport();
  const { data: runningGames } = useRunningGames();
  const running = useSyncRunning();
  const { data: retiredIds = [] } = useRetiredVersions();
  const { data: monitoring } = useMonitoringSettings();
  const { data: here } = useDevice();
  const unrecorded = useUnrecorded(id);
  const now = useNow(60_000);

  // Rewinds, switches and slot changes all rewrite the live save folder: one at a time.
  const [busy, setBusy] = useState(false);
  const [rewinding, setRewinding] = useState(false);
  const [landedId, setLandedId] = useState<number | null>(null);
  const [openForks, setOpenForks] = useState<ReadonlySet<number>>(() => new Set());

  const retired = useMemo(() => new Set(retiredIds), [retiredIds]);
  const versions = useMemo(
    () => (game?.versions ?? []).filter((v) => v.slot_id === game?.active_slot?.id),
    [game],
  );
  const activeId = game?.active_slot?.active_version_id ?? null;
  const status = report.data?.games.find((s) => s.game_uuid === game?.uuid);
  const pendingId = pendingRemoteVersion(status, retired);
  const gapMs = sessionGapMs(monitoring?.version_cooldown_minutes ?? 0);
  const layout = useMemo(
    () => layoutSessions({ versions, activeId, gapMs, pendingId }),
    [versions, activeId, gapMs, pendingId],
  );
  const graph = useMemo(
    () => (search.view === 'graph' ? layoutGraph({ versions, mine: layout.lineage, gapMs }) : null),
    [search.view, versions, layout, gapMs],
  );

  if (isLoading) return <GameDetailSkeleton />;

  if (error !== null || game === undefined) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4">
        <div className="font-bold text-err">{t.errors.loadFailed}</div>
        <button type="button" className={btn} onClick={() => { void refetch(); }}>{t.errors.retry}</button>
      </div>
    );
  }

  const gameRunning = runningGames?.includes(game.id) === true;
  // Everything that rewrites the save folder is all or nothing, and fails when the game holds its files.
  const writeFailed = (message: string, err: unknown): void => {
    const kind = typeof err === 'object' && err !== null ? (err as Partial<Failure>).kind : undefined;
    if (kind === 'restore_incomplete') {
      reportError(message, err, t.game.puttingBack);
      return;
    }
    const held = gameRunning || kind === 'game_running' || kind === 'saves_in_use';
    reportError(message, err, held ? t.game.closeAndRetry(game.title) : t.game.closeIfOpen);
  };
  const syncOn = typeof settings?.remote_path === 'string' && settings.remote_path !== '';
  const chipState = syncChip({ on: syncOn, running: running || report.isFetching, reported: report.data !== undefined, status });
  const deviceOf = (version: Version): ReturnType<typeof checkpointDevice> => checkpointDevice(version, here, report.data);
  const remoteKind = report.data?.devices.find((d) => d.name === status?.remote_device)?.kind ?? 'desktop';
  const head = layout.lineage.length > 0 ? layout.lineage[layout.lineage.length - 1] : null;
  const selected = versions.find((v) => v.id === search.v) ?? head;
  const onLine = selected !== null && layout.lineage.some((v) => v.id === selected.id);
  // Every timeline counts: a rewind doesn't take back the time you played.
  const sessions = splitSessions(versions, gapMs).length;
  const lessPlay = selected === null || !onLine ? 0 : playedMs(layout.lineage.slice(layout.lineage.findIndex((v) => v.id === selected.id) + 1), versions, gapMs);

  const refresh = (): Promise<unknown> => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['game', id] }),
    queryClient.invalidateQueries({ queryKey: ['games'] }),
  ]);
  const select = (v: number | undefined): void => {
    void navigate({ search: (prev) => ({ ...prev, v }), replace: true });
  };

  async function rewind(version: Version): Promise<void> {
    setBusy(true);
    setRewinding(true);
    const streaks = new Promise((resolve) => setTimeout(resolve, REWIND_MS));
    try {
      await restoreVersion(version.id);
      await Promise.all([refresh(), streaks]);
      setLandedId(version.id);
      select(undefined);
    } catch (err) {
      writeFailed(t.game.failed.rewind, err);
    } finally {
      setRewinding(false);
      setBusy(false);
    }
  }

  async function continueHere(): Promise<void> {
    if (pendingId === null) return;
    setBusy(true);
    try {
      await applyRemoteVersion(pendingId);
      await Promise.all([refresh(), report.refetch()]);
      setLandedId(pendingId);
      select(undefined);
    } catch (err) {
      writeFailed(t.game.failed.apply, err);
    } finally {
      setBusy(false);
    }
  }

  async function slotChange(change: () => Promise<unknown>, failed: string): Promise<void> {
    setBusy(true);
    try {
      await change();
      await refresh();
      setOpenForks(new Set());
      select(undefined);
    } catch (err) {
      writeFailed(failed, err);
    } finally {
      setBusy(false);
    }
  }

  function onSyncChip(): void {
    if (chipState.kind === 'played-too') {
      // Unfold the other device's timeline where it left yours.
      const theirs = layout.days.flatMap((d) => d.rows.flatMap((r) => r.forks))
        .filter((f) => f.line.some((v) => !deviceOf(v).here))
        .map((f) => f.root.id);
      setOpenForks(new Set([...openForks, ...theirs]));
      return;
    }
    if (chipState.kind === 'new-from') return;
    void report.refetch().then((result) => {
      if (result.isError) reportError(t.sync.failed, result.error);
    });
  }

  function toggleFork(rootId: number): void {
    const next = new Set(openForks);
    if (!next.delete(rootId)) next.add(rootId);
    setOpenForks(next);
  }

  const drawer = search.v !== undefined;
  const selectedId = selected?.id ?? null;
  const onSelect = (v: number): void => { select(v); };
  const order = graph === null ? navOrder(layout, openForks) : graphNavOrder(graph, versions);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <GameHeader
        game={game}
        running={gameRunning}
        sync={chipState}
        inCloud={syncOn && report.data?.remote_games.some((g) => g.uuid === game.uuid) === true}
        syncDevice={remoteKind}
        onSyncChip={onSyncChip}
        played={versions.length === 0 ? null : formatDuration(totalPlayedMs(versions, gapMs), t)}
        sessions={sessions}
        view={search.view ?? 'sessions'}
        onView={(view) => { void navigate({ search: (prev) => ({ ...prev, view: view === 'graph' ? view : undefined }) }); }}
        busy={busy || gameRunning}
        onSwitchSlot={(slotId) => {
          if (slotId !== game.active_slot?.id) void slotChange(() => switchSlot(game.id, slotId), t.game.failed.switchSlot);
        }}
        onCreateSlot={() => { void slotChange(() => createSlot(game.id, t.game.playthroughName(game.slots.length + 1)), t.game.failed.newSlot); }}
      />
      <div className="flex min-h-0 flex-1 gap-4 px-8">
        {versions.length === 0 ? (
          <EmptyGame title={game.title} />
        ) : (
          <TimelineFrame order={order} selectedId={selectedId} activeId={activeId} onSelect={onSelect} rewinding={rewinding}>
            <AnimatePresence initial={false}>
              {layout.ghost !== null && pendingId !== null && (
                <GhostRow
                  key="ghost"
                  versions={layout.ghost}
                  device={deviceOf(layout.ghost[0])}
                  onContinue={() => { void continueHere(); }}
                  busy={busy}
                  running={gameRunning}
                  now={now}
                />
              )}
            </AnimatePresence>
            {/* The old view stays for its exit, out of the flow, so its dots hand over to the new ones and fly. */}
            <AnimatePresence mode="popLayout" initial={false}>
              {graph === null ? (
                <motion.div key="sessions" exit={{ opacity: 0 }} transition={{ duration: 0.2 }}>
                  <SessionList
                    game={game}
                    layout={layout}
                    versions={versions}
                    gapMs={gapMs}
                    activeId={activeId}
                    selectedId={selectedId}
                    onSelect={onSelect}
                    openForks={openForks}
                    onToggleFork={toggleFork}
                    deviceOf={deviceOf}
                    landedId={landedId}
                    unrecorded={unrecorded}
                    running={gameRunning}
                    now={now}
                  />
                </motion.div>
              ) : (
                <motion.div key="graph" exit={{ opacity: 0 }} transition={{ duration: 0.2 }}>
                  <GraphView
                    game={game}
                    graph={graph}
                    versions={versions}
                    activeId={activeId}
                    selectedId={selectedId}
                    onSelect={onSelect}
                    deviceOf={deviceOf}
                    landedId={landedId}
                    now={now}
                  />
                </motion.div>
              )}
            </AnimatePresence>
          </TimelineFrame>
        )}
        {selected !== null && (
          <aside
            aria-label={t.checkpoint.panel}
            className={cn(
              'w-[336px] shrink-0 overflow-y-auto pb-8',
              'max-[999px]:fixed max-[999px]:bottom-4 max-[999px]:right-4 max-[999px]:z-20 max-[999px]:max-h-[calc(100%-2rem)] max-[999px]:rounded-[20px] max-[999px]:pb-0 max-[999px]:shadow-[0_24px_60px_oklch(0%_0_0/0.55),0_0_0_1px_oklch(100%_0_0/0.06)]',
              !drawer && 'max-[999px]:hidden',
            )}
          >
            <CheckpointPanel
              game={game}
              version={selected}
              active={head}
              unrecorded={unrecorded}
              onLine={onLine}
              device={deviceOf(selected)}
              lessPlayMs={lessPlay}
              aheadDevice={pendingId === null ? null : (status?.remote_device ?? t.sessions.anotherDevice)}
              busy={busy}
              running={gameRunning}
              onName={(tag) => { void nameCheckpoint(id, selected.id, tag, t.game.failed.name); }}
              onRewind={() => { void rewind(selected); }}
              onClose={() => { select(undefined); }}
              now={now}
            />
          </aside>
        )}
      </div>
    </div>
  );
}

function GameDetailSkeleton(): ReactElement {
  return (
    <div className="flex h-full animate-pulse flex-col">
      <header className="flex items-center gap-5 px-8 pb-6 pt-6">
        <Skeleton className="size-[72px] rounded-[22px]" />
        <div className="flex-1 space-y-3">
          <Skeleton className="h-9 w-64" />
          <Skeleton className="h-[30px] w-80 rounded-full" />
        </div>
      </header>
      <div className="flex flex-1 gap-4 px-8">
        <div className="flex-1 space-y-2">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={`skeleton-${i.toString()}`} className="h-14 w-full rounded-[14px]" />
          ))}
        </div>
        <Skeleton className="h-64 w-[336px] rounded-[20px] max-[999px]:hidden" />
      </div>
    </div>
  );
}

function Skeleton({ className }: { className: string }): ReactElement {
  return <div className={cn('rounded-md bg-el', className)} />;
}

import { UseQueryResult, useQuery } from '@tanstack/react-query';
import { getGame, getGames, getRetiredVersions, getRunningGames, getSyncSettings, initialSync, refreshSync } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import type { SyncSettings } from '@/ipc/bindings/SyncSettings';
import { setSyncOutcome } from './sync-activity';

export const useGames = (): UseQueryResult<FullGame[]> => {
  return useQuery({
    queryKey: ['games'],
    queryFn: getGames,
  });
};

export const useGame = (gameId: number): UseQueryResult<FullGame> => {
  return useQuery({
    queryKey: ['game', gameId],
    queryFn: () => getGame(gameId),
    enabled: !!gameId,
  });
};

export const useSyncSettings = (): UseQueryResult<SyncSettings> => {
  return useQuery({ queryKey: ['sync-settings'], queryFn: getSyncSettings });
};

let startupShared = false;

/** The backend caches its startup report for the whole run: refetching it would undo later changes. */
function nextSync(): Promise<SyncReport> {
  if (startupShared) return refreshSync();
  startupShared = true;
  return initialSync();
}

/**
 * The last sync report. The first request shares the backend's startup sync;
 * the cache is then fed by the backend's periodic `sync-complete` events.
 * Explicit refreshes and invalidations run a new sync. Disabled while sync is off.
 */
export const useSyncReport = (): UseQueryResult<SyncReport> => {
  const { data: settings } = useSyncSettings();
  const remote = settings?.remote_path;
  return useQuery({
    queryKey: ['sync-report'],
    queryFn: () => nextSync().then(
      (report) => { setSyncOutcome(null); return report; },
      (err: unknown) => { setSyncOutcome(err); throw err; },
    ),
    enabled: typeof remote === 'string' && remote !== '',
    retry: false,
    staleTime: Infinity,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
  });
};

/** Explicit sync: update the same query's data, pending state and errors. */
export const refreshSyncReport = (): Promise<SyncReport> => queryClient.query({
  queryKey: ['sync-report'],
  queryFn: () => refreshSync().then(
    (report) => { setSyncOutcome(null); return report; },
    (err: unknown) => { setSyncOutcome(err); throw err; },
  ),
  staleTime: 0,
  retry: false,
  meta: { quiet: true },
});

export const useRetiredVersions = (): UseQueryResult<string[]> => {
  return useQuery({ queryKey: ['retired-versions'], queryFn: getRetiredVersions });
};

/** Ids of the games running now, read once; `game-started` and `game-stopped` keep it current. */
export const useRunningGames = (): UseQueryResult<number[]> => {
  return useQuery({
    queryKey: ['running-games'],
    queryFn: getRunningGames,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
};

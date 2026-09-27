import { UseQueryResult, useQuery } from '@tanstack/react-query';
import { getGames, getGame, scanInstalled, getScanRoots } from './games';
import { FullGame } from '@/models/game';
import { InstallScannerItem } from '@/models/install_scanner_item';
import { getSyncSettings, refreshSync } from './sync';
import { SyncReport, SyncSettings } from '@/models/sync';
import { getRetiredVersions } from './pruning';

export const useGames = (): UseQueryResult<FullGame[]> => {
  return useQuery({
    queryKey: ['games'],
    queryFn: getGames,
  });
};

export const useGame = (gameId: number): UseQueryResult<FullGame | undefined> => {
  return useQuery({
    queryKey: ['game', gameId],
    queryFn: () => getGame(gameId),
    enabled: !!gameId,
  });
};

export const useScannedGames = (enabled = true): UseQueryResult<InstallScannerItem[]> => {
  return useQuery({
    queryKey: ['scanned-games'],
    queryFn: async () => {
      const [games, installed] = await Promise.all([getGames(), scanInstalled()]);
      return installed.filter(i => !games.find(g => g.title === i.title));
    },
    enabled,
  });
};

export const useScanRoots = (): UseQueryResult<string[]> => {
  return useQuery({
    queryKey: ['scan-roots'],
    queryFn: getScanRoots,
  });
};

export const useSyncSettings = (): UseQueryResult<SyncSettings> => {
  return useQuery({ queryKey: ['sync-settings'], queryFn: getSyncSettings });
};

/**
 * The last sync report. It syncs once when first needed; after that the
 * cache is fed by the backend's periodic `sync-complete` events, and callers
 * `refetch()` only on explicit user action. Disabled while sync is off.
 */
export const useSyncReport = (): UseQueryResult<SyncReport> => {
  const { data: settings } = useSyncSettings();
  const remote = settings?.remote_path;
  return useQuery({
    queryKey: ['sync-report'],
    queryFn: refreshSync,
    enabled: typeof remote === 'string' && remote !== '',
    retry: false,
    staleTime: Infinity,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
  });
};

export const useRetiredVersions = (): UseQueryResult<string[]> => {
  return useQuery({ queryKey: ['retired-versions'], queryFn: getRetiredVersions });
};

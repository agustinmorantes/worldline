import { UseQueryResult, useQuery } from '@tanstack/react-query';
import { getGames, getGame, scanInstalled, getScanRoots } from './games';
import { FullGame } from '@/models/game';
import { InstallScannerItem } from '@/models/install_scanner_item';

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

import { UseQueryResult, useQuery } from '@tanstack/react-query';
import { getGames, getScanRoots, scanInstalled } from '@/ipc/commands';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';

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

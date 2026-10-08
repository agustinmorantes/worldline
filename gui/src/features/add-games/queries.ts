import { UseQueryResult, useQuery } from '@tanstack/react-query';
import { getScanRoots, scanInstalled } from '@/ipc/commands';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';

/** The install scan. It reads every library on disk, so it only runs once asked to. */
export const useInstalledScan = (enabled: boolean): UseQueryResult<InstallScannerItem[]> => {
  return useQuery({
    queryKey: ['scan-installed'],
    queryFn: scanInstalled,
    enabled,
    retry: false,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
};

export const useScanRoots = (): UseQueryResult<string[]> => {
  return useQuery({
    queryKey: ['scan-roots'],
    queryFn: getScanRoots,
  });
};

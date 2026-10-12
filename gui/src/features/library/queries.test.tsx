import { act, renderHook, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { queryClient } from '@/app/query-client';
import { initialSync, refreshSync } from '@/ipc/commands';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import { refreshSyncReport, useSyncReport } from './queries';

vi.mock('@/ipc/commands', () => ({
  getSyncSettings: vi.fn().mockResolvedValue({ remote_path: '/remote' }),
  initialSync: vi.fn(),
  refreshSync: vi.fn(),
}));

const startup: SyncReport = {
  synced_at: '2026-10-12T06:00:00Z', games: [], remote_games: [],
  devices: [], remote_versions: {}, problems: [],
};

beforeEach(() => {
  queryClient.clear();
  vi.mocked(initialSync).mockReset().mockResolvedValue(startup);
  vi.mocked(refreshSync).mockReset();
});
afterEach(() => { queryClient.clear(); });

it('shares startup reports and performs a fresh sync on explicit refresh', async () => {
  const { result } = renderHook(() => {
    const report = useSyncReport();
    return { data: report.data, isFetching: report.isFetching, isError: report.isError };
  }, {
    wrapper: ({ children }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>,
  });
  await waitFor(() => { expect(result.current.data).toEqual(startup); });
  expect(initialSync).toHaveBeenCalledOnce();
  expect(refreshSync).not.toHaveBeenCalled();

  const fresh = { ...startup, synced_at: '2026-10-12T06:05:00Z' };
  let complete: (report: SyncReport) => void = () => undefined;
  vi.mocked(refreshSync).mockReturnValue(new Promise((resolve) => { complete = resolve; }));
  let pending: Promise<SyncReport>;
  act(() => { pending = refreshSyncReport(); });
  await waitFor(() => { expect(result.current.isFetching).toBe(true); });
  await act(async () => { complete(fresh); await pending; });
  await waitFor(() => { expect(result.current.data).toEqual(fresh); });
  expect(refreshSync).toHaveBeenCalledOnce();
  expect(initialSync).toHaveBeenCalledOnce();

  const failure = { kind: 'sync_folder_unreachable', message: 'offline' };
  vi.mocked(refreshSync).mockRejectedValue(failure);
  await act(async () => { await expect(refreshSyncReport()).rejects.toEqual(failure); });
  await waitFor(() => { expect(result.current.isError).toBe(true); });
  expect(result.current.data).toEqual(fresh);
});

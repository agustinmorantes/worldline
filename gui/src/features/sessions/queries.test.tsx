import type { ReactNode } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { previewPruning } from '@/ipc/commands';
import type { PruningPreview } from '@/ipc/bindings/PruningPreview';
import { useCleanupWouldRemove } from './queries';

vi.mock('@/ipc/commands', () => ({ previewPruning: vi.fn() }));

const retiring = (...uuids: string[]): PruningPreview => ({ retire_versions: uuids }) as unknown as PruningPreview;

describe('useCleanupWouldRemove', () => {
  it('asks afresh on each opening, checking again instead of showing the last answer', async () => {
    // Like the app's: answers stay fresh for minutes.
    const client = new QueryClient({ defaultOptions: { queries: { staleTime: 5 * 60_000, retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }): ReactNode => <QueryClientProvider client={client}>{children}</QueryClientProvider>;
    let answer = (_: PruningPreview): void => undefined;
    vi.mocked(previewPruning)
      .mockResolvedValueOnce(retiring('v'))
      .mockReturnValueOnce(new Promise((resolve) => { answer = resolve; }));

    const { result, rerender } = renderHook(
      ({ opening, open }: { opening: number; open: boolean }) => useCleanupWouldRemove('v', opening, open),
      { wrapper, initialProps: { opening: 1, open: true } },
    );
    await waitFor(() => { expect(result.current.data).toBe(true); });

    rerender({ opening: 1, open: false });
    rerender({ opening: 2, open: true });
    expect(result.current.isPending).toBe(true);
    expect(result.current.data).toBeUndefined();
    answer(retiring());
    await waitFor(() => { expect(result.current.data).toBe(false); });
    expect(previewPruning).toHaveBeenCalledTimes(2);
    expect(previewPruning).toHaveBeenLastCalledWith(undefined, 'v');
  });
});

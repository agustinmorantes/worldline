import { QueryClientProvider } from '@tanstack/react-query';
import { act, render, screen, waitFor } from '@testing-library/react';
import type { ReactElement } from 'react';
import { describe, expect, it, vi } from 'vitest';
import type { DeletedItem } from '@/ipc/bindings/DeletedItem';
import { recentlyDeleted } from '@/ipc/commands';
import { useRecentlyDeleted } from '@/features/settings/queries';
import { queryClient } from './query-client';
import { listenNotifications } from './events';

const handlers = vi.hoisted(() => new Map<string, (payload: unknown) => void>());
vi.mock('@/ipc/events', () => ({
  on: (name: string, handler: (payload: unknown) => void) => {
    handlers.set(name, handler);
    return Promise.resolve(() => undefined);
  },
}));
vi.mock('@/ipc/commands', () => ({
  getGame: vi.fn(),
  getRecoveredRestores: vi.fn(() => Promise.resolve(0)),
  getStartupNotice: vi.fn(() => Promise.resolve(null)),
  recentlyDeleted: vi.fn(),
}));

const celeste: DeletedItem = {
  kind: 'game', uuid: 'g1', title: 'Celeste', game_title: 'Celeste',
  deleted_at: new Date().toISOString(), expires_at: new Date(Date.now() + 29 * 86_400_000).toISOString(),
};

function Titles(): ReactElement {
  const { data = [] } = useRecentlyDeleted();
  return <p data-testid="titles">{data.map((item) => item.title).join(',')}</p>;
}

describe('sync-complete', () => {
  it('refreshes Recently deleted as other devices delete and restore', async () => {
    vi.mocked(recentlyDeleted).mockResolvedValue([]);
    listenNotifications();
    render(<QueryClientProvider client={queryClient}><Titles /></QueryClientProvider>);
    await waitFor(() => { expect(recentlyDeleted).toHaveBeenCalledTimes(1); });
    const synced = (): void => { act(() => { handlers.get('sync-complete')?.({}); }); };

    // Another device deletes the game; the next sync brings it here.
    vi.mocked(recentlyDeleted).mockResolvedValue([celeste]);
    synced();
    await waitFor(() => { expect(screen.getByTestId('titles')).toHaveTextContent('Celeste'); });

    // Another device restores it.
    vi.mocked(recentlyDeleted).mockResolvedValue([]);
    synced();
    await waitFor(() => { expect(screen.getByTestId('titles')).toBeEmptyDOMElement(); });
  });
});

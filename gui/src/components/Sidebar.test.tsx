import { fireEvent, render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Sidebar } from './Sidebar';
import type { FullGame } from '@/models/game';

vi.mock('@/api/queries', () => ({
  useGames: vi.fn(),
}));

vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement>) =>
    <a {...props}>{children}</a>,
  useRouter: vi.fn(() => ({ navigate: vi.fn() })),
}));

vi.mock('./AddGameModal', () => ({
  AddGameModal: () => null,
}));

import { useGames } from '@/api/queries';

function mockQuery(data: FullGame[] | undefined, opts: { isLoading?: boolean; error?: Error | null } = {}) {
  vi.mocked(useGames).mockReturnValue({
    data,
    isLoading: opts.isLoading ?? false,
    error: opts.error ?? null,
    isError: opts.error !== null && opts.error !== undefined,
    isSuccess: data !== undefined,
    isPending: opts.isLoading ?? false,
    isFetching: false,
    isRefetching: false,
    refetch: vi.fn(),
    status: opts.isLoading === true ? 'pending' : 'success',
    fetchStatus: 'idle',
  } as unknown as ReturnType<typeof useGames>);
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});

describe('Sidebar', () => {
  it('renders the library header', () => {
    mockQuery(undefined, { isLoading: true });
    render(<Sidebar />);
    expect(screen.getByText('Library')).toBeInTheDocument();
  });

  it('renders skeleton rows while loading, defaulting to 3', () => {
    mockQuery(undefined, { isLoading: true });
    render(<Sidebar />);
    const skeletonRows = document.querySelectorAll('.flex.items-center.gap-3.px-3.py-2');
    expect(skeletonRows.length).toBeGreaterThanOrEqual(3);
  });

  it('reads skeleton count from localStorage', () => {
    localStorage.setItem('games-count', '5');
    mockQuery(undefined, { isLoading: true });
    render(<Sidebar />);
    const skeletonRows = document.querySelectorAll('.flex.items-center.gap-3.px-3.py-2');
    expect(skeletonRows.length).toBeGreaterThanOrEqual(5);
  });

  it('renders an error message when query fails', () => {
    mockQuery(undefined, { error: new Error('network') });
    render(<Sidebar />);
    expect(screen.getByText(/error loading games/i)).toBeInTheDocument();
  });

  it('renders game titles when loaded', () => {
    const games = [
      { id: 1, title: 'Elden Ring', tracked_dirs: [], versions: [], slots: [], active_version_id: null },
      { id: 2, title: 'Dark Souls', tracked_dirs: [], versions: [], slots: [], active_version_id: null },
    ] as unknown as FullGame[];
    mockQuery(games);
    render(<Sidebar />);
    expect(screen.getByText('Elden Ring')).toBeInTheDocument();
    expect(screen.getByText('Dark Souls')).toBeInTheDocument();
  });

  it('shows Import Games and Settings nav items', () => {
    mockQuery([]);
    render(<Sidebar />);
    expect(screen.getByText('Import Games')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('shows Help & Tour and dispatches the onboarding event', () => {
    mockQuery([]);
    const dispatchSpy = vi.spyOn(window, 'dispatchEvent');
    render(<Sidebar />);

    fireEvent.click(screen.getByRole('button', { name: /help & tour/i }));

    expect(screen.getByRole('button', { name: /help & tour/i })).toBeInTheDocument();
    expect(dispatchSpy).toHaveBeenCalledTimes(1);
    expect(dispatchSpy.mock.calls[0]?.[0]).toBeInstanceOf(CustomEvent);
    expect(dispatchSpy.mock.calls[0]?.[0].type).toBe('open-onboarding');
  });
});

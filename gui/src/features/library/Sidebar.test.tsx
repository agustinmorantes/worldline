import { fireEvent, render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Sidebar } from './Sidebar';
import { type ShellNav, ShellNavContext } from './shell-nav';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { makeGame } from '@/ipc/builders';

vi.mock('@/features/library/queries', () => ({
  useGames: vi.fn(),
  useSyncSettings: vi.fn(),
  useSyncReport: vi.fn(),
}));

vi.mock('@tanstack/react-router', () => ({
  Link: ({ children, to: _to, params: _params, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement> & { to?: string; params?: unknown }) =>
    <a href="#" {...props}>{children}</a>,
}));

import { useGames, useSyncReport, useSyncSettings } from '@/features/library/queries';

function mockGames(data: FullGame[] | undefined, opts: { isLoading?: boolean; error?: Error | null } = {}) {
  vi.mocked(useGames).mockReturnValue({
    data,
    isLoading: opts.isLoading ?? false,
    error: opts.error ?? null,
  } as unknown as ReturnType<typeof useGames>);
}

function mockSync(remotePath: string | null, unadopted = 0) {
  vi.mocked(useSyncSettings).mockReturnValue({
    data: { remote_path: remotePath },
  } as unknown as ReturnType<typeof useSyncSettings>);
  vi.mocked(useSyncReport).mockReturnValue({
    data: remotePath === null ? undefined : {
      synced_at: new Date().toISOString(),
      games: [],
      remote_games: Array.from({ length: unadopted }, (_, i) => ({ adopted: false, uuid: `r${i.toString()}` })),
    },
    isFetching: false,
    isError: false,
    refetch: vi.fn(),
  } as unknown as ReturnType<typeof useSyncReport>);
}

function renderSidebar(nav: Partial<ShellNav> = {}): void {
  const value: ShellNav = { current: null, pending: null, go: vi.fn(), flying: { home: null, out: null }, ...nav };
  render(<ShellNavContext value={value}><Sidebar /></ShellNavContext>);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockSync(null);
});

describe('Sidebar', () => {
  it('renders the library label and skeleton rows while loading', () => {
    mockGames(undefined, { isLoading: true });
    renderSidebar();
    expect(screen.getByRole('complementary', { name: 'Library' })).toBeInTheDocument();
    expect(document.querySelectorAll('.animate-pulse').length).toBeGreaterThanOrEqual(3);
  });

  it('renders an error message when the query fails', () => {
    mockGames(undefined, { error: new Error('network') });
    renderSidebar();
    expect(screen.getByText(/couldn’t load this/i)).toBeInTheDocument();
  });

  it('renders game titles and marks the open game', () => {
    mockGames([makeGame({ id: 1, title: 'Elden Ring' }), makeGame({ id: 2, title: 'Dark Souls' })]);
    renderSidebar({ current: { view: 'game', gameId: 2 } });
    expect(screen.getByRole('link', { name: /elden ring/i })).not.toHaveAttribute('aria-current');
    expect(screen.getByRole('link', { name: /dark souls/i })).toHaveAttribute('aria-current', 'page');
  });

  it('navigates through the shell on click', () => {
    mockGames([makeGame({ id: 7, title: 'Hades' })]);
    const go = vi.fn();
    renderSidebar({ go });
    fireEvent.click(screen.getByRole('link', { name: /hades/i }));
    expect(go).toHaveBeenCalledWith({ view: 'game', gameId: 7 });
  });

  it('shows Add games and Settings, and offers to turn on cloud when sync is off', () => {
    mockGames([]);
    const go = vi.fn();
    renderSidebar({ go });
    expect(screen.getByRole('link', { name: /add games/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /settings/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /turn on cloud/i }));
    expect(go).toHaveBeenCalledWith({ view: 'settings' });
  });

  it('badges Add games with cloud games not on this PC', () => {
    mockGames([]);
    mockSync('/remote', 2);
    renderSidebar();
    expect(screen.getByText('2 games in your cloud aren’t on this PC yet')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /sync now/i })).toBeInTheDocument();
  });
});

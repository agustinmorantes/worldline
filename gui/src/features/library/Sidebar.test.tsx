import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { Sidebar } from './Sidebar';
import { type ShellNav, ShellNavContext } from './shell-nav';
import { setSyncOutcome } from './sync-activity';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { makeGame } from '@/ipc/builders';
import { TooltipProvider } from '@/shared/ui/Tooltip';

vi.mock('@/features/library/queries', () => ({
  useGames: vi.fn(),
  useSyncSettings: vi.fn(),
  useSyncReport: vi.fn(),
  useRunningGames: vi.fn(() => ({ data: [] })),
}));

vi.mock('@tanstack/react-router', () => ({
  useRouter: () => ({ navigate: vi.fn() }),
  Link: ({ children, to: _to, params: _params, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement> & { to?: string; params?: unknown }) =>
    <a href="#" {...props}>{children}</a>,
}));

import { useGames, useRunningGames, useSyncReport, useSyncSettings } from '@/features/library/queries';

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
  const value: ShellNav = { current: null, pending: null, go: vi.fn(), land: vi.fn(), flying: { home: null, out: null, landing: [] }, ...nav };
  render(<TooltipProvider><ShellNavContext value={value}><Sidebar /></ShellNavContext></TooltipProvider>);
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

  it('opens the game’s menu on right-click, without navigating', () => {
    mockGames([makeGame({ id: 7, title: 'Hades' })]);
    const go = vi.fn();
    renderSidebar({ go });
    fireEvent.contextMenu(screen.getByRole('link', { name: /hades/i }));
    expect(screen.getByRole('menuitem', { name: 'Rename game' })).toBeInTheDocument();
    expect(screen.getByRole('menuitem', { name: 'Delete everywhere' })).toBeInTheDocument();
    expect(go).not.toHaveBeenCalled();
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

  it('says Recording on a running game only', () => {
    mockGames([makeGame({ id: 1, title: 'Elden Ring' }), makeGame({ id: 2, title: 'Dark Souls' })]);
    vi.mocked(useRunningGames).mockReturnValueOnce({ data: [2] } as unknown as ReturnType<typeof useRunningGames>);
    renderSidebar();
    expect(screen.getByRole('link', { name: /dark souls/i })).toHaveTextContent('Recording');
    expect(screen.getByRole('link', { name: /elden ring/i })).not.toHaveTextContent('Recording');
  });
});

describe('the sync card', () => {
  function mockReport(report: { data?: object; isFetching?: boolean; isError?: boolean }): void {
    vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: '/remote' } } as unknown as ReturnType<typeof useSyncSettings>);
    vi.mocked(useSyncReport).mockReturnValue({
      data: undefined, isFetching: false, isError: false, refetch: vi.fn(), ...report,
    } as unknown as ReturnType<typeof useSyncReport>);
  }
  const synced = { synced_at: new Date().toISOString(), games: [], remote_games: [] };

  afterEach(() => { setSyncOutcome(null); });

  it.each([
    ['synced', { data: synced }, /synced/i, 'lucide-cloud-check'],
    ['syncing', { data: synced, isFetching: true }, /syncing/i, 'lucide-cloud'],
    ['failed', { data: synced, isError: true }, /sync failed/i, 'lucide-cloud-alert'],
    ['idle', {}, /not synced yet/i, 'lucide-cloud'],
  ] as const)('shows the %s state with its own icon', (state, report, status, icon) => {
    mockGames([]);
    mockReport(report);
    renderSidebar();
    const card = screen.getByRole('button', { name: /sync now/i });
    expect(card).toHaveAttribute('data-state', state);
    expect(card).toHaveAccessibleName(status);
    expect(card.querySelector(`svg.${icon}`)).not.toBeNull();
  });

  it('shows an unreachable folder with the cloud-off icon', () => {
    mockGames([]);
    mockReport({ isError: true });
    setSyncOutcome({ kind: 'sync_folder_unreachable', message: 'gone' });
    renderSidebar();
    const card = screen.getByRole('button', { name: /sync now/i });
    expect(card).toHaveAttribute('data-state', 'unavailable');
    expect(card).toHaveAccessibleName(/cloud folder not available/i);
    expect(card.querySelector('svg.lucide-cloud-off')).not.toBeNull();
  });

  it('hints at retrying when a sync failed', async () => {
    mockGames([]);
    mockReport({ isError: true });
    renderSidebar();
    fireEvent.pointerMove(screen.getByRole('button', { name: /sync now/i }));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Something went wrong. Click to try again.');
  });
});

// The test double of `motion` remounts its elements on every render, so the field is looked up afresh each time.
describe('library search', () => {
  const games = (): FullGame[] => [
    makeGame({ id: 1, title: 'Elden Ring' }),
    makeGame({ id: 2, title: 'Pokémon Legends' }),
    makeGame({ id: 3, title: 'Hades' }),
  ];
  const library = (): HTMLElement => screen.getByRole('complementary', { name: 'Library' });

  it('filters by title, ignoring accents, and says when nothing matches', () => {
    mockGames(games());
    renderSidebar();
    fireEvent.click(screen.getByRole('button', { name: 'Search the library' }));
    expect(screen.getByRole('searchbox', { name: 'Search the library' })).toHaveFocus();

    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'pokemon' } });
    expect(within(library()).getByRole('link', { name: /pokémon legends/i })).toBeInTheDocument();
    expect(within(library()).queryByRole('link', { name: /elden ring/i })).toBeNull();

    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'zelda' } });
    expect(screen.getByText('No games match')).toBeInTheDocument();
  });

  it('closes on Escape and shows every game again', () => {
    mockGames(games());
    renderSidebar();
    fireEvent.click(screen.getByRole('button', { name: 'Search the library' }));
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'hades' } });
    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Escape' });
    expect(screen.queryByRole('searchbox')).toBeNull();
    expect(screen.getAllByRole('link', { name: /elden ring|pokémon|hades/i })).toHaveLength(3);
  });

  it('opens the first match on Enter', () => {
    mockGames(games());
    const go = vi.fn();
    renderSidebar({ go });
    fireEvent.click(screen.getByRole('button', { name: 'Search the library' }));
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'e' } });
    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Enter' });
    expect(go).toHaveBeenCalledWith({ view: 'game', gameId: 1 });
  });

  it('opens with Ctrl+F or a slash, and when typing in the sidebar', () => {
    mockGames(games());
    renderSidebar();
    fireEvent.keyDown(window, { key: 'f', ctrlKey: true });
    expect(screen.getByRole('searchbox')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Escape' });

    fireEvent.keyDown(window, { key: '/' });
    expect(screen.getByRole('searchbox')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole('searchbox'), { key: 'Escape' });

    fireEvent.keyDown(screen.getByRole('link', { name: /hades/i }), { key: 'h' });
    expect(screen.getByRole('searchbox')).toHaveValue('h');
  });
});

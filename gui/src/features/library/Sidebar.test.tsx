import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
  refreshSyncReport: vi.fn().mockResolvedValue(undefined),
  useRunningGames: vi.fn(() => ({ data: [] })),
}));

const navigate = vi.hoisted(() => vi.fn());
vi.mock('@tanstack/react-router', () => ({
  useRouter: () => ({ navigate }),
  Link: ({ children, to: _to, params: _params, ...props }: React.AnchorHTMLAttributes<HTMLAnchorElement> & { to?: string; params?: unknown }) =>
    <a href="#" {...props}>{children}</a>,
}));

vi.mock('@/ipc/commands', () => ({ deleteGames: vi.fn(), recoverGames: vi.fn(), untrackGames: vi.fn() }));
vi.mock('sonner', () => ({ toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() } }));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));

import { useGames, useRunningGames, useSyncReport, useSyncSettings } from '@/features/library/queries';
import { deleteGames, recoverGames, untrackGames } from '@/ipc/commands';
import { toast } from 'sonner';
import { reportError } from '@/shared/errors/report';
import type { GameResult } from '@/ipc/bindings/GameResult';

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

function renderSidebar(nav: Partial<ShellNav> = {}): (nav: Partial<ShellNav>) => void {
  const value = (more: Partial<ShellNav>): ShellNav => ({ current: null, pending: null, go: vi.fn(), land: vi.fn(), flying: { home: null, out: null, landing: [] }, ...nav, ...more });
  const tree = (more: Partial<ShellNav>) => <TooltipProvider><ShellNavContext value={value(more)}><Sidebar /></ShellNavContext></TooltipProvider>;
  const { rerender } = render(tree({}));
  return (more) => { rerender(tree(more)); };
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

  it('asks for an update when another device synced with a newer Worldline', async () => {
    mockGames([]);
    mockReport({ isError: true });
    setSyncOutcome({ kind: 'update_required', message: 'newer' });
    renderSidebar();
    const card = screen.getByRole('button', { name: /sync now/i });
    expect(card).toHaveAttribute('data-state', 'update');
    expect(card).toHaveAccessibleName(/update worldline to sync/i);
    fireEvent.pointerMove(card);
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Update Worldline on this PC to keep syncing.');
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

describe('picking several games', () => {
  // Sorted by title, as none has been played: Celeste, Hades, Ori, Tunic.
  const games = (): FullGame[] => [
    makeGame({ id: 4, title: 'Tunic', uuid: 'tunic' }),
    makeGame({ id: 2, title: 'Hades', uuid: 'hades' }),
    makeGame({ id: 3, title: 'Ori', uuid: 'ori' }),
    makeGame({ id: 1, title: 'Celeste', uuid: 'celeste' }),
  ];
  const row = (title: string): HTMLElement => screen.getByRole('link', { name: new RegExp(`^${title}`) });
  const picked = (): string[] => games()
    .map((game) => game.title)
    .filter((title) => row(title).textContent?.endsWith('Selected') === true)
    .sort();
  const result = (game: FullGame, outcome: GameResult['outcome']): GameResult => ({ uuid: game.uuid, game_id: game.id, title: game.title, outcome });
  const done = { outcome: 'done' } as const;
  const failed = { outcome: 'failed', failure: { kind: null, message: 'disk full', details: [] } } as unknown as GameResult['outcome'];

  /** The games, with Hades and Ori already in the cloud. */
  function setup(nav: Partial<ShellNav> = {}): { go: ReturnType<typeof vi.fn>; navTo: (nav: Partial<ShellNav>) => void } {
    mockGames(games());
    vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: '/remote' } } as unknown as ReturnType<typeof useSyncSettings>);
    vi.mocked(useSyncReport).mockReturnValue({
      data: { synced_at: new Date().toISOString(), games: [], remote_games: [{ adopted: true, uuid: 'hades' }, { adopted: true, uuid: 'ori' }] },
      isFetching: false, isError: false, refetch: vi.fn(),
    } as unknown as ReturnType<typeof useSyncReport>);
    const go = vi.fn();
    const navTo = renderSidebar({ go, ...nav });
    return { go, navTo };
  }

  it('picks with Ctrl or ⌘ instead of letting the webview open the link elsewhere', () => {
    const { go } = setup();
    // `fireEvent` returns false once a handler has prevented the default: no new window, no download.
    expect(fireEvent.click(row('Hades'), { ctrlKey: true })).toBe(false);
    expect(fireEvent.click(row('Tunic'), { metaKey: true })).toBe(false);
    expect(fireEvent.click(row('Ori'), { shiftKey: true, ctrlKey: true })).toBe(false);
    expect(go).not.toHaveBeenCalled();
    expect(picked()).toEqual(['Hades', 'Ori', 'Tunic']);
    expect(screen.getByRole('status')).toHaveTextContent('3 games selected. Press Escape to clear.');
    // A middle click stays in place too.
    expect(fireEvent(row('Celeste'), new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 1 }))).toBe(false);
    expect(fireEvent.click(screen.getByRole('link', { name: /settings/i }), { ctrlKey: true })).toBe(false);
  });

  it('takes a Shift run in the order the list shows, from the game on screen', () => {
    setup({ current: { view: 'game', gameId: 1 } });
    fireEvent.click(row('Ori'), { shiftKey: true });
    expect(picked()).toEqual(['Celeste', 'Hades', 'Ori']);
    fireEvent.click(row('Hades'), { ctrlKey: true });
    expect(picked()).toEqual(['Celeste', 'Ori']);
  });

  it('drops the picks on a plain click, on Escape and on going elsewhere', () => {
    const { go, navTo } = setup();
    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Ori'), { ctrlKey: true });
    expect(fireEvent.click(row('Tunic'))).toBe(false);
    expect(go).toHaveBeenCalledWith({ view: 'game', gameId: 4 });
    expect(picked()).toEqual([]);

    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Ori'), { ctrlKey: true });
    fireEvent.keyDown(row('Ori'), { key: 'Escape' });
    expect(picked()).toEqual([]);

    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Ori'), { ctrlKey: true });
    navTo({ current: { view: 'settings' } });
    expect(picked()).toEqual([]);
  });

  it('right-clicking one of the picks offers plural actions; any other game keeps its own menu', () => {
    setup();
    fireEvent.click(row('Celeste'), { ctrlKey: true });
    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Ori'), { ctrlKey: true });
    fireEvent.contextMenu(row('Hades'));
    expect(screen.getByText('3 games selected')).toBeInTheDocument();
    const remove = screen.getByRole('menuitem', { name: /Remove 2 games from this PC/ });
    expect(remove).toHaveTextContent('Celeste stays: not in your cloud yet.');
    expect(screen.getByRole('menuitem', { name: 'Delete 3 games everywhere' })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });

    fireEvent.contextMenu(row('Tunic'));
    expect(screen.getByRole('menuitem', { name: 'Delete everywhere' })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(picked()).toEqual([]);
  });

  it('removes only the games in the cloud from this PC, and names the ones that stay', async () => {
    setup();
    const [, hades, ori] = games();
    vi.mocked(untrackGames).mockResolvedValue([result(hades, done), result(ori, done)]);
    fireEvent.click(row('Celeste'), { ctrlKey: true });
    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Ori'), { ctrlKey: true });
    fireEvent.contextMenu(row('Ori'));
    fireEvent.click(screen.getByRole('menuitem', { name: /Remove 2 games from this PC/ }));
    expect(untrackGames).toHaveBeenCalledTimes(1);
    expect(untrackGames).toHaveBeenCalledWith([2, 3]);
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith('2 games are off this PC. Their history is still in your cloud.', { description: 'Celeste stays: not in your cloud yet.' });
    });
    expect(reportError).not.toHaveBeenCalled();
  });

  it('deletes nothing until the listed games are confirmed', () => {
    setup();
    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Tunic'), { ctrlKey: true });
    fireEvent.contextMenu(row('Hades'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete 2 games everywhere' }));
    const dialog = screen.getByRole('dialog', { name: 'Delete 2 games everywhere?' });
    expect(within(dialog).getAllByRole('listitem').map((item) => item.lastChild?.textContent)).toEqual(['Hades', 'Tunic']);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(deleteGames).not.toHaveBeenCalled();
    expect(picked()).toEqual(['Hades', 'Tunic']);
  });

  it('deletes them all in one call, with one Undo that brings back only what was deleted', async () => {
    setup({ current: { view: 'game', gameId: 2 } });
    const [tunic, hades, ori] = games();
    vi.mocked(deleteGames).mockResolvedValue([result(hades, done), result(ori, failed), result(tunic, done)]);
    fireEvent.click(row('Ori'), { ctrlKey: true });
    fireEvent.click(row('Tunic'), { ctrlKey: true });
    fireEvent.contextMenu(row('Ori'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete 3 games everywhere' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete 3 games' }));

    await waitFor(() => { expect(toast.success).toHaveBeenCalledTimes(1); });
    expect(deleteGames).toHaveBeenCalledTimes(1);
    expect(deleteGames).toHaveBeenCalledWith([2, 3, 4]);
    // The open game was among them, so the screen leaves it.
    expect(navigate).toHaveBeenCalledWith({ to: '/' });
    const [message, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [string, { action: { onClick: () => void } }];
    expect(message).toBe('2 games deleted');
    // The one that failed is named, and a retry asks only for it.
    const [failure, , , retry] = vi.mocked(reportError).mock.calls[0] as unknown as [string, unknown, string, { onClick: () => void }];
    expect(failure).toBe('Couldn’t delete Ori.');
    vi.mocked(deleteGames).mockResolvedValue([result(ori, done)]);
    retry.onClick();
    expect(deleteGames).toHaveBeenLastCalledWith([3]);

    vi.mocked(recoverGames).mockResolvedValue([result(hades, done), result(tunic, failed)]);
    options.action.onClick();
    expect(recoverGames).toHaveBeenCalledTimes(1);
    expect(recoverGames).toHaveBeenCalledWith(['hades', 'tunic']);
    await waitFor(() => {
      expect(reportError).toHaveBeenCalledWith('Couldn’t bring back Tunic.', expect.anything(), 'They’re still in Settings, under Recently deleted.', expect.anything());
    });
    const undoRetry = vi.mocked(reportError).mock.lastCall?.[3] as unknown as { onClick: () => void };
    undoRetry.onClick();
    expect(recoverGames).toHaveBeenLastCalledWith(['tunic']);
  });

  it('stays on the open game when it was not among the deleted', async () => {
    setup({ current: { view: 'game', gameId: 1 } });
    const [tunic, hades] = games();
    vi.mocked(deleteGames).mockResolvedValue([result(hades, done), result(tunic, done)]);
    // Ctrl starts from the open game; take it out again.
    fireEvent.click(row('Hades'), { ctrlKey: true });
    fireEvent.click(row('Celeste'), { ctrlKey: true });
    fireEvent.click(row('Tunic'), { ctrlKey: true });
    fireEvent.contextMenu(row('Tunic'));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete 2 games everywhere' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete 2 games' }));
    await waitFor(() => { expect(toast.success).toHaveBeenCalledWith('2 games deleted', expect.anything()); });
    expect(deleteGames).toHaveBeenCalledWith([2, 4]);
    expect(navigate).not.toHaveBeenCalled();
  });
});

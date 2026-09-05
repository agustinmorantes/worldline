import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { FullGame } from '@/models/game';
import { Slot } from '@/models/slot';
import { Version } from '@/models/version';

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => (options: Record<string, unknown>) => ({
    ...options,
    useParams: () => ({ gameId: '1' }),
  }),
  useRouter: () => ({ navigate: vi.fn() }),
}));

vi.mock('@/api/queries', () => ({
  useGame: vi.fn(),
}));

// The timeline is mocked so the test can assert on what it is *handed*.
// Rendering it would only let us count nodes, and a version rendered for the
// wrong slot looks identical to one rendered for the right slot.
const timelineProps = vi.fn();
vi.mock('@/components/BranchingTimeline', () => ({
  BranchingTimeline: (props: { versions: Version[]; activeVersionId?: number }) => {
    timelineProps(props);
    return <div data-testid="timeline" />;
  },
}));

import { useGame } from '@/api/queries';
import { GameDetail } from '@/routes/game/$gameId';

function slot(id: number, name: string, activeVersionId?: number): Slot {
  const s = new Slot(name);
  s.id = id;
  s.active_version_id = activeVersionId;
  return s;
}

function version(id: number, slotId: number): Version {
  const v = new Version(id, slotId);
  v.id = id;
  v.created_at = new Date('2026-01-01T00:00:00Z');
  return v;
}

function mockGame(game: Partial<FullGame>): void {
  vi.mocked(useGame).mockReturnValue({
    data: game as FullGame,
    isLoading: false,
    error: null,
    refetch: vi.fn(),
  } as unknown as ReturnType<typeof useGame>);
}

describe('GameDetail timeline scoping', () => {
  it('hands the timeline only the active slot versions', () => {
    const active = slot(1, 'Default Slot', 11);
    mockGame({
      id: 1,
      title: 'Elden Ring',
      tracked_dirs: [],
      slots: [active, slot(2, 'Challenge Run', 22)],
      active_slot: active,
      versions: [version(11, 1), version(12, 1), version(22, 2)],
    });

    render(<GameDetail />);

    expect(timelineProps).toHaveBeenCalledTimes(1);
    const props = timelineProps.mock.calls[0][0] as { versions: Version[]; activeVersionId?: number };
    expect(props.versions.map((v) => v.id)).toEqual([11, 12]);
    // The pointer lives on the slot; the game has no active version of its own.
    expect(props.activeVersionId).toBe(11);
  });

  it('shows the empty state when the active slot has no versions, even though another slot does', () => {
    const active = slot(2, 'Challenge Run');
    mockGame({
      id: 1,
      title: 'Elden Ring',
      tracked_dirs: [],
      slots: [slot(1, 'Default Slot', 11), active],
      active_slot: active,
      versions: [version(11, 1)],
    });

    render(<GameDetail />);

    expect(screen.queryByTestId('timeline')).not.toBeInTheDocument();
    expect(screen.getByText(/set up monitoring for elden ring/i)).toBeInTheDocument();
  });
});

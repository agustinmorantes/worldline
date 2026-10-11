import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Game } from '@/ipc/bindings/Game';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { addAll, onGameAdded, useAddRun } from './add-run';

vi.mock('@/ipc/commands', () => ({
  saveGame: vi.fn(),
  setGameIcon: vi.fn(),
}));
vi.mock('@/app/query-client', () => ({ queryClient: { invalidateQueries: vi.fn().mockResolvedValue(undefined) } }));
vi.mock('@/shared/errors/report', () => ({ reportError: vi.fn() }));
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }));

import { saveGame } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';

const item = (title: string): InstallScannerItem => ({
  title,
  dirs: [`C:/saves/${title}`],
  candidate_dirs: [],
  install_path: null,
  icon_path: null,
  last_modified_at: null,
  source: 'steam',
});

/** `saveGame` resolving only when the test says so. */
function deferredSaves(): { finish: () => void } {
  const pending: (() => void)[] = [];
  let id = 0;
  vi.mocked(saveGame).mockImplementation((form) => new Promise<Game>((resolve) => {
    pending.push(() => { resolve({ id: ++id, title: form.title, icon_path: null } as Game); });
  }));
  return { finish: () => { pending.shift()?.(); } };
}

describe('addAll', () => {
  beforeEach(() => {
    vi.mocked(saveGame).mockReset();
    vi.mocked(reportError).mockClear();
  });

  it('adds each game once, even when asked again while it runs', async () => {
    vi.mocked(saveGame).mockImplementation((form) => Promise.resolve({ id: 1, title: form.title, icon_path: null } as Game));
    const items = [item('Balatro'), item('Hades')];
    await Promise.all([addAll(items), addAll(items)]);
    expect(vi.mocked(saveGame).mock.calls.map(([form]) => form.title)).toEqual(['Balatro', 'Hades']);
    expect(reportError).not.toHaveBeenCalled();
  });

  it('shows which game is being added and how far along the run is, to any view that looks', async () => {
    const saves = deferredSaves();
    const { result } = renderHook(() => useAddRun());
    let done!: Promise<void>;
    act(() => { done = addAll([item('Balatro'), item('Hades')]); });
    expect(result.current).toMatchObject({ total: 2, current: 'Balatro', running: true });
    expect(result.current?.added.size).toBe(0);

    // A view mounted mid-run sees the same run.
    const later = renderHook(() => useAddRun());
    expect(later.result.current?.current).toBe('Balatro');

    await act(async () => { saves.finish(); await Promise.resolve(); });
    await vi.waitFor(() => { expect(result.current?.current).toBe('Hades'); });
    expect([...(result.current?.added ?? [])]).toEqual(['Balatro']);

    await act(async () => { saves.finish(); await done; });
    expect(result.current).toMatchObject({ running: false, current: null });
    expect(result.current?.added.size).toBe(2);
  });

  it('tells listeners about each added game, and keeps going after a failure', async () => {
    vi.mocked(saveGame)
      .mockRejectedValueOnce({ kind: 'name_taken', message: 'taken' })
      .mockResolvedValueOnce({ id: 7, title: 'Hades', icon_path: null } as Game);
    const added = vi.fn();
    const stop = onGameAdded(added);
    await addAll([item('Balatro'), item('Hades')]);
    stop();
    expect(reportError).toHaveBeenCalledTimes(1);
    expect(added).toHaveBeenCalledWith('Hades', expect.objectContaining({ id: 7 }));
  });
});

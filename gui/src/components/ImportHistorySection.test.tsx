import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  confirm: vi.fn(),
  inspect: vi.fn(),
  importHistory: vi.fn(),
  invalidateQueries: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open, confirm: mocks.confirm }));
vi.mock('@tanstack/react-query', () => ({
  useQueryClient: () => ({ invalidateQueries: mocks.invalidateQueries }),
}));
vi.mock('sonner', () => ({ toast: { success: mocks.toastSuccess, error: mocks.toastError } }));
vi.mock('@/api/history', () => ({
  inspectHistoryBundle: mocks.inspect,
  importGameHistory: mocks.importHistory,
}));

import { ImportHistorySection } from '@/components/ImportHistorySection';

const trackedUuid = '11111111-1111-4111-8111-111111111111';
const summary = {
  game_uuid: '22222222-2222-4222-8222-222222222222',
  title: 'Example Game',
  slot_count: 2,
  version_count: 3,
  tracked_dir_count: 1,
  blob_count: 2,
  retired_version_count: 1,
  active_tracked_dirs: [{ uuid: trackedUuid, label: 'Saves', source_path_hint: 'D:/old/Saves' }],
};

describe('ImportHistorySection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.open.mockResolvedValue('/portable/history');
    mocks.confirm.mockResolvedValue(true);
    mocks.inspect.mockResolvedValue(summary);
    mocks.importHistory.mockResolvedValue({
      game_uuid: summary.game_uuid,
      title: summary.title,
      slot_count: summary.slot_count,
      version_count: summary.version_count,
      tracked_dir_count: summary.tracked_dir_count,
      blob_count: summary.blob_count,
      retired_version_count: summary.retired_version_count,
    });
    mocks.invalidateQueries.mockResolvedValue(undefined);
  });

  it('inspects a bundle, accepts a destination mapping, then confirms and imports', async () => {
    render(<ImportHistorySection />);

    fireEvent.click(screen.getByRole('button', { name: 'Choose and Inspect Bundle' }));
    expect(await screen.findByText('Example Game')).toBeInTheDocument();
    expect(mocks.inspect).toHaveBeenCalledWith('/portable/history');
    expect(screen.getByText('Source path hint: D:/old/Saves')).toBeInTheDocument();

    const importButton = screen.getByRole('button', { name: 'Import History' });
    expect(importButton).toBeEnabled();
    fireEvent.change(screen.getByRole('textbox', { name: 'Destination for Saves' }), {
      target: { value: 'C:/Games/Example/Saves' },
    });
    expect(importButton).toBeEnabled();

    fireEvent.click(importButton);
    await waitFor(() => {
      expect(mocks.importHistory).toHaveBeenCalledWith('/portable/history', {
        [trackedUuid]: 'C:/Games/Example/Saves',
      });
    });
    expect(mocks.confirm).toHaveBeenCalledWith(
      expect.stringContaining('live save files and active slot/version pointers are left untouched'),
      { title: 'Import History', kind: 'warning' },
    );
    expect(mocks.invalidateQueries).toHaveBeenCalledWith({ queryKey: ['games'] });
    expect(mocks.toastSuccess).toHaveBeenCalledWith('Imported 3 version(s) for Example Game');
  });

  it('does not leave a previously inspected bundle available after a failed inspection', async () => {
    render(<ImportHistorySection />);
    fireEvent.click(screen.getByRole('button', { name: 'Choose and Inspect Bundle' }));
    expect(await screen.findByText('Example Game')).toBeInTheDocument();

    mocks.open.mockResolvedValueOnce('/portable/broken');
    mocks.inspect.mockRejectedValueOnce(new Error('invalid bundle'));
    fireEvent.click(screen.getByRole('button', { name: 'Choose and Inspect Bundle' }));
    await waitFor(() => { expect(mocks.toastError).toHaveBeenCalled(); });
    expect(screen.queryByRole('button', { name: 'Import History' })).not.toBeInTheDocument();
    expect(mocks.importHistory).not.toHaveBeenCalled();
  });

  it('lets core reuse existing game paths without redundant mappings', async () => {
    render(<ImportHistorySection />);
    fireEvent.click(screen.getByRole('button', { name: 'Choose and Inspect Bundle' }));
    expect(await screen.findByText('Example Game')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Import History' }));
    await waitFor(() => {
      expect(mocks.importHistory).toHaveBeenCalledWith('/portable/history', {});
    });
  });
});

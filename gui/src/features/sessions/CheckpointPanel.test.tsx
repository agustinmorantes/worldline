import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeVersion } from '@/ipc/builders';
import { CheckpointPanel, ScreenshotSlot, zoomBox } from './CheckpointPanel';

vi.mock('./queries', () => ({
  useScreenshot: vi.fn(),
  useCleanupWouldRemove: vi.fn(),
}));

vi.mock('@/ipc/commands', () => ({ tagVersion: vi.fn(() => Promise.resolve()) }));

// Holding is a timed pointer gesture; a click stands for a completed hold.
vi.mock('@/shared/ui/HoldButton', () => ({
  HoldButton: ({ label, onComplete }: { label: string; onComplete: () => void }) => (
    <button type='button' onClick={onComplete}>{label}</button>
  ),
}));

import { useCleanupWouldRemove, useScreenshot } from './queries';
import { tagVersion } from '@/ipc/commands';
import { nameCheckpoint } from './name-checkpoint';

function setup(path: string | null | undefined): void {
  vi.mocked(useScreenshot).mockReturnValue({ data: path } as unknown as ReturnType<typeof useScreenshot>);
  render(<ScreenshotSlot versionId={7} tint="var(--acc)" />);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(useCleanupWouldRemove).mockReturnValue({ isPending: true } as ReturnType<typeof useCleanupWouldRemove>);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ScreenshotSlot', () => {
  it('asks for the checkpoint’s own screenshot', () => {
    setup(null);
    expect(useScreenshot).toHaveBeenCalledWith(7);
  });

  it('is only the tint line when the checkpoint has no screenshot', () => {
    setup(null);
    expect(document.querySelector('img')).toBeNull();
  });

  it('is only the tint line while the screenshot is loading', () => {
    setup(undefined);
    expect(document.querySelector('img')).toBeNull();
  });

  it('loads the file lazily through the asset protocol', () => {
    setup('C:/data/repository/games/g/slots/s/versions/v/screenshot.jpg');
    const img = document.querySelector('img');
    expect(img).toHaveAttribute('src', 'asset://C:/data/repository/games/g/slots/s/versions/v/screenshot.jpg');
    expect(img).toHaveAttribute('loading', 'lazy');
    expect(img).toHaveAttribute('decoding', 'async');
  });

  it('can only be enlarged once the image has loaded', () => {
    setup('C:/shot.jpg');
    expect(screen.queryByRole('button', { name: 'Enlarge the screenshot' })).not.toBeInTheDocument();
    fireEvent.load(screen.getByAltText('Screenshot of this checkpoint'));
    expect(screen.getByRole('button', { name: 'Enlarge the screenshot' })).toBeInTheDocument();
  });

  it('enlarges the image over the app, lifting it out of the card', () => {
    vi.spyOn(HTMLImageElement.prototype, 'naturalWidth', 'get').mockReturnValue(1280);
    vi.spyOn(HTMLImageElement.prototype, 'naturalHeight', 'get').mockReturnValue(720);
    setup('C:/shot.jpg');
    fireEvent.load(screen.getByAltText('Screenshot of this checkpoint'));
    fireEvent.click(screen.getByRole('button', { name: 'Enlarge the screenshot' }));

    const dialog = screen.getByRole('dialog', { name: 'Screenshot of this checkpoint' });
    expect(within(dialog).getByRole('img')).toHaveAttribute('src', 'asset://C:/shot.jpg');
    // The test's motion stub remounts the card's image on every render: look it up again.
    const thumb = screen.getAllByAltText('Screenshot of this checkpoint').find((img) => !dialog.contains(img));
    expect(thumb).toHaveStyle({ visibility: 'hidden' });
  });

  it('keeps the image from assistive tech until it has loaded', () => {
    setup('C:/shot.jpg');
    expect(screen.queryByRole('img', { name: 'Screenshot of this checkpoint' })).not.toBeInTheDocument();
    fireEvent.load(screen.getByAltText('Screenshot of this checkpoint'));
    expect(screen.getByRole('img', { name: 'Screenshot of this checkpoint' })).toBeInTheDocument();
  });
});

describe('CheckpointPanel on the checkpoint you are on', () => {
  const NOW = new Date(Date.UTC(2026, 9, 10, 12, 0));

  function setup(unrecorded: { dueAt: Date } | null): () => void {
    vi.mocked(useScreenshot).mockReturnValue({ data: null } as unknown as ReturnType<typeof useScreenshot>);
    const active = makeVersion({ id: 3 });
    const onRewind = vi.fn();
    render(
      <CheckpointPanel
        game={makeGame({ id: 1, title: 'Hades' })}
        version={active}
        active={active}
        unrecorded={unrecorded}
        onLine
        device={{ here: true, name: null, kind: 'desktop' }}
        lessPlayMs={0}
        aheadDevice={null}
        busy={false}
        running={false}
        onName={vi.fn()}
        onRewind={onRewind}
        now={NOW}
      />,
    );
    return onRewind;
  }

  it('says you are here and offers no rewind while the saves match it', () => {
    setup(null);
    expect(screen.getByText('You are here')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Hold/ })).not.toBeInTheDocument();
    expect(screen.queryByText('Not recorded yet')).not.toBeInTheDocument();
  });

  it('offers to go back to it while its saves changed unrecorded', () => {
    setup({ dueAt: new Date(NOW.getTime() + 12 * 60_000) });
    expect(screen.getByText('Go back to')).toBeInTheDocument();
    expect(screen.queryByText('You are here')).not.toBeInTheDocument();
    expect(screen.getByText('Not recorded yet')).toBeInTheDocument();
    expect(screen.getByText('in ~12 min')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Hold to go back' })).toBeInTheDocument();
    expect(screen.getByText('Your progress since then is kept as a checkpoint first.')).toBeInTheDocument();
  });

  it('rewinds to it when held', () => {
    const onRewind = setup({ dueAt: NOW });
    fireEvent.click(screen.getByRole('button', { name: 'Hold to go back' }));
    expect(onRewind).toHaveBeenCalledOnce();
  });
});

describe('zoomBox', () => {
  it('fills the window inside the margin at the image’s aspect, centred', () => {
    expect(zoomBox({ width: 1280, height: 720 }, { width: 1064, height: 800 })).toEqual({
      left: 32, top: 400 - 562.5 / 2, width: 1000, height: 562.5,
    });
  });

  it('never grows past the image’s natural size', () => {
    expect(zoomBox({ width: 1280, height: 720 }, { width: 2560, height: 1440 })).toEqual({
      left: 640, top: 360, width: 1280, height: 720,
    });
  });
});


describe('CheckpointPanel name', () => {
  const version = makeVersion({ id: 3, parent_id: 2, tag: 'Before the boss' });
  type Removal = Partial<ReturnType<typeof useCleanupWouldRemove>>;

  // Wired to the IPC the way the game screen wires it.
  function setup(tag: string | null, removal: Removal = { isPending: false, isError: false, data: false }): void {
    vi.mocked(useScreenshot).mockReturnValue({ data: null } as unknown as ReturnType<typeof useScreenshot>);
    vi.mocked(useCleanupWouldRemove).mockReturnValue(removal as ReturnType<typeof useCleanupWouldRemove>);
    const shown = { ...version, tag };
    render(
      <CheckpointPanel
        game={makeGame({ id: 1, title: 'Hades' })}
        version={shown}
        active={shown}
        unrecorded={null}
        onLine
        device={{ here: true, name: null, kind: 'desktop' }}
        lessPlayMs={0}
        aheadDevice={null}
        busy={false}
        running={false}
        onName={(next) => { void nameCheckpoint(1, shown.id, next, 'Couldn’t name the checkpoint.'); }}
        onRewind={vi.fn()}
        now={new Date(Date.UTC(2026, 9, 10, 12, 0))}
      />,
    );
  }

  const field = (): HTMLElement => screen.getByRole('textbox', { name: 'Name this checkpoint' });
  const removeButton = (): HTMLElement => screen.getByRole('button', { name: 'Remove name' });
  const dialog = (): HTMLElement => screen.getByRole('dialog', { name: 'Remove the name “Before the boss”?' });

  // Enter blurs the field, and leaving it saves: a blur stands for both.
  function clearByHand(): void {
    fireEvent.change(field(), { target: { value: '  ' } });
    fireEvent.blur(field());
  }

  it('offers to remove a name only when there is one', () => {
    setup(null);
    expect(screen.queryByRole('button', { name: 'Remove name' })).not.toBeInTheDocument();
  });

  it('removes the name with an empty tag once confirmed, then focuses the field', async () => {
    setup('Before the boss');
    fireEvent.click(removeButton());
    expect(dialog()).toHaveTextContent('cleanup can remove it like any other checkpoint');
    expect(tagVersion).not.toHaveBeenCalled();
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Remove name' }));
    expect(tagVersion).toHaveBeenCalledExactlyOnceWith(3, '');
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(field()).toHaveFocus();
  });

  it('keeps the name and sends nothing when cancelled, focus back on the button', async () => {
    setup('Before the boss');
    fireEvent.click(removeButton());
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(removeButton()).toHaveFocus();
    expect(tagVersion).not.toHaveBeenCalled();
  });

  it('keeps the name on Escape, focus back on the button', async () => {
    setup('Before the boss');
    fireEvent.click(removeButton());
    fireEvent.keyDown(dialog(), { key: 'Escape' });
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(removeButton()).toHaveFocus();
    expect(tagVersion).not.toHaveBeenCalled();
  });

  it('asks before a name cleared by hand goes, and puts it back when cancelled', async () => {
    setup('Before the boss');
    clearByHand();
    expect(dialog()).toBeInTheDocument();
    expect(tagVersion).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => { expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); });
    expect(field()).toHaveValue('Before the boss');
    expect(field()).toHaveFocus();
    fireEvent.blur(field());
    expect(tagVersion).not.toHaveBeenCalled();
  });

  it('removes a name cleared by hand once, when confirmed', async () => {
    setup('Before the boss');
    clearByHand();
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Remove name' }));
    await waitFor(() => { expect(field()).toHaveFocus(); });
    // Leaving the still-empty field before the new name arrives neither asks nor saves again.
    fireEvent.blur(field());
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(tagVersion).toHaveBeenCalledExactlyOnceWith(3, '');
  });

  it('saves a new name at once', () => {
    setup('Before the boss');
    fireEvent.change(field(), { target: { value: 'After the boss' } });
    fireEvent.blur(field());
    expect(tagVersion).toHaveBeenCalledExactlyOnceWith(3, 'After the boss');
  });

  it('asks about the next cleanup only while the dialog is open, for this checkpoint', () => {
    setup('Before the boss');
    expect(useCleanupWouldRemove).toHaveBeenLastCalledWith(version.uuid, false);
    fireEvent.click(removeButton());
    expect(useCleanupWouldRemove).toHaveBeenLastCalledWith(version.uuid, true);
  });

  it('warns when the next cleanup would remove it', () => {
    setup('Before the boss', { isPending: false, isError: false, data: true });
    fireEvent.click(removeButton());
    expect(dialog()).toHaveTextContent('The next cleanup will remove it, and it can’t be brought back after that.');
  });

  it('adds no warning when the next cleanup would keep it', () => {
    setup('Before the boss', { isPending: false, isError: false, data: false });
    fireEvent.click(removeButton());
    expect(dialog()).not.toHaveTextContent('next cleanup');
  });

  it('says so while it checks, and when it couldn’t check', () => {
    setup('Before the boss', { isPending: true, isError: false, data: undefined });
    fireEvent.click(removeButton());
    expect(dialog()).toHaveTextContent('Checking the next cleanup…');
    cleanup();
    setup('Before the boss', { isPending: false, isError: true, data: undefined });
    fireEvent.click(removeButton());
    expect(dialog()).toHaveTextContent('Couldn’t check whether the next cleanup would remove it.');
  });
});

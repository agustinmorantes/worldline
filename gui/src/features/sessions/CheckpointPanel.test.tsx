import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeVersion } from '@/ipc/builders';
import { CheckpointPanel, ScreenshotSlot, zoomBox } from './CheckpointPanel';

vi.mock('./queries', () => ({
  useScreenshot: vi.fn(),
}));

// Holding is a timed pointer gesture; a click stands for a completed hold.
vi.mock('@/shared/ui/HoldButton', () => ({
  HoldButton: ({ label, onComplete }: { label: string; onComplete: () => void }) => (
    <button type='button' onClick={onComplete}>{label}</button>
  ),
}));

import { useScreenshot } from './queries';

function setup(path: string | null | undefined): void {
  vi.mocked(useScreenshot).mockReturnValue({ data: path } as unknown as ReturnType<typeof useScreenshot>);
  render(<ScreenshotSlot versionId={7} tint="var(--acc)" />);
}

beforeEach(() => {
  vi.clearAllMocks();
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
  function setup(tag: string | null): ReturnType<typeof vi.fn> {
    vi.mocked(useScreenshot).mockReturnValue({ data: null } as unknown as ReturnType<typeof useScreenshot>);
    const version = makeVersion({ id: 3, tag });
    const onName = vi.fn();
    render(
      <CheckpointPanel
        game={makeGame({ id: 1, title: 'Hades' })}
        version={version}
        active={version}
        unrecorded={null}
        onLine
        device={{ here: true, name: null, kind: 'desktop' }}
        lessPlayMs={0}
        aheadDevice={null}
        busy={false}
        running={false}
        onName={onName}
        onRewind={vi.fn()}
        now={new Date(Date.UTC(2026, 9, 10, 12, 0))}
      />,
    );
    return onName;
  }

  it('offers to remove a name only when there is one', () => {
    setup(null);
    expect(screen.queryByRole('button', { name: 'Remove name' })).not.toBeInTheDocument();
  });

  it('removes the name with an empty tag once confirmed', () => {
    const onName = setup('Before the boss');
    fireEvent.click(screen.getByRole('button', { name: 'Remove name' }));
    const dialog = screen.getByRole('dialog', { name: 'Remove the name “Before the boss”?' });
    expect(dialog).toHaveTextContent('cleanup can remove it like any other checkpoint');
    expect(onName).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove name' }));
    expect(onName).toHaveBeenCalledExactlyOnceWith('');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('keeps the name when cancelled', () => {
    const onName = setup('Before the boss');
    fireEvent.click(screen.getByRole('button', { name: 'Remove name' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onName).not.toHaveBeenCalled();
  });
});

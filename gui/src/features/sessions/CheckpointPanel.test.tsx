import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeVersion } from '@/ipc/builders';
import { CheckpointPanel, ScreenshotSlot } from './CheckpointPanel';

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

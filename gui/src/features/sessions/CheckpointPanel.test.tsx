import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ScreenshotSlot } from './CheckpointPanel';

vi.mock('./queries', () => ({
  useScreenshot: vi.fn(),
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

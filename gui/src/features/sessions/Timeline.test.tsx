import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeVersion } from '@/ipc/builders';
import type { Version } from '@/ipc/bindings/Version';
import { TooltipProvider } from '@/shared/ui/Tooltip';
import { Dot } from './Timeline';

vi.mock('./queries', () => ({
  useScreenshotPreview: vi.fn(),
}));

import { useScreenshotPreview } from './queries';

function setup(version: Version, path: string | null | undefined = 'C:/shot.jpg'): HTMLElement {
  vi.mocked(useScreenshotPreview).mockReturnValue({ data: path } as unknown as ReturnType<typeof useScreenshotPreview>);
  render(
    <TooltipProvider>
      <Dot
        game={makeGame({ id: 1, title: 'Hades' })}
        version={version}
        left={0}
        side={false}
        elsewhere={false}
        active={false}
        selected={false}
        landed={false}
        onSelect={vi.fn()}
        now={new Date()}
      />
    </TooltipProvider>,
  );
  return screen.getByRole('button');
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Dot preview', () => {
  it('asks for nothing until its tooltip opens', async () => {
    const dot = setup(makeVersion({ id: 7, tag: 'Boss' }));
    expect(useScreenshotPreview).not.toHaveBeenCalled();
    fireEvent.pointerMove(dot);
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Boss');
    expect(useScreenshotPreview).toHaveBeenCalledWith(7);
  });

  it('shows the screenshot over the label once it has loaded', async () => {
    fireEvent.pointerMove(setup(makeVersion({ id: 7 })));
    await screen.findByRole('tooltip');
    const img = document.querySelector('img[src="asset://C:/shot.jpg"]');
    if (img === null) throw new Error('no screenshot');
    expect(img).toHaveClass('hidden');
    fireEvent.load(img);
    expect(img).not.toHaveClass('hidden');
  });

  it('is only the label without a screenshot', async () => {
    fireEvent.pointerMove(setup(makeVersion({ id: 7 }), null));
    await screen.findByRole('tooltip');
    expect(document.querySelector('img')).toBeNull();
  });

  it('never asks for a cleaned-up checkpoint’s screenshot', async () => {
    fireEvent.pointerMove(setup(makeVersion({ id: 7, retired: true })));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Cleaned up');
    expect(useScreenshotPreview).not.toHaveBeenCalled();
  });
});

import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { makeGame, makeVersion, naive } from '@/ipc/builders';
import type { Version } from '@/ipc/bindings/Version';
import { TooltipProvider } from '@/shared/ui/Tooltip';
import { SessionList } from './SessionList';
import { layoutSessions } from './sessions';
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

describe('Pending dot', () => {
  const NOW = new Date(Date.UTC(2026, 9, 10, 12, 0));
  const MIN = 60_000;

  function setup(unrecorded: { dueAt: Date } | null, running = false): void {
    vi.mocked(useScreenshotPreview).mockReturnValue({ data: null } as unknown as ReturnType<typeof useScreenshotPreview>);
    const versions = [
      makeVersion({ id: 1, created_at: naive(new Date(NOW.getTime() - 30 * MIN)) }),
      makeVersion({ id: 2, parent_id: 1, created_at: naive(new Date(NOW.getTime() - 5 * MIN)), tag: 'Boss' }),
    ];
    render(
      <TooltipProvider>
        <SessionList
          game={makeGame({ id: 1, title: 'Hades' })}
          layout={layoutSessions({ versions, activeId: 2, gapMs: 40 * MIN })}
          versions={versions}
          gapMs={40 * MIN}
          activeId={2}
          selectedId={null}
          onSelect={vi.fn()}
          openForks={new Set()}
          onToggleFork={vi.fn()}
          deviceOf={() => ({ here: true, name: null, kind: 'desktop' })}
          landedId={null}
          unrecorded={unrecorded}
          running={running}
          now={NOW}
        />
      </TooltipProvider>,
    );
  }

  it('shows only while the saves changed unrecorded', () => {
    setup(null);
    expect(screen.queryByRole('img', { name: /Not recorded yet/ })).not.toBeInTheDocument();
  });

  it('sits after the checkpoint you are on', () => {
    setup({ dueAt: new Date(NOW.getTime() + 12 * MIN) });
    const pending = screen.getByRole('img', { name: /Not recorded yet/ });
    const active = screen.getByRole('button', { name: /^Boss,/ });
    expect(active.compareDocumentPosition(pending) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('says when it is recorded, in minutes', async () => {
    setup({ dueAt: new Date(NOW.getTime() + 12 * MIN) });
    fireEvent.pointerMove(screen.getByRole('img', { name: /Not recorded yet/ }));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Not recorded yet · recorded in ~12 min');
  });

  it('says a few seconds under a minute', () => {
    setup({ dueAt: new Date(NOW.getTime() + 20_000) });
    expect(screen.getByRole('img', { name: 'Not recorded yet · recorded in a few seconds' })).toBeInTheDocument();
  });

  it('says closing the game records it while the game is running', () => {
    setup({ dueAt: new Date(NOW.getTime() + 3 * MIN) }, true);
    expect(screen.getByRole('img', { name: 'Not recorded yet · recorded in ~3 min or when you close the game' })).toBeInTheDocument();
  });
});

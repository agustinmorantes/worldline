import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EmptyTimelineState } from './EmptyTimelineState';

describe('EmptyTimelineState', () => {
  it('explains that monitoring is active when tracked folders exist', () => {
    render(
      <EmptyTimelineState
        gameTitle="Elden Ring"
        trackedDirCount={2}
      />
    );

    expect(screen.getByText(/timeline is watching elden ring/i)).toBeInTheDocument();
    expect(screen.getByText(/2 monitored folders/i)).toBeInTheDocument();
    expect(screen.getByText(/waiting for the first save event/i)).toBeInTheDocument();
  });

  it('prompts for setup when no folders are tracked yet', () => {
    render(
      <EmptyTimelineState
        gameTitle="Cyberpunk 2077"
        trackedDirCount={0}
      />
    );

    expect(screen.getByText(/set up monitoring for cyberpunk 2077/i)).toBeInTheDocument();
    expect(screen.getByText(/no tracked folders yet/i)).toBeInTheDocument();
  });
});

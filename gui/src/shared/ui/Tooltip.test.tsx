import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Tooltip, TooltipProvider } from './Tooltip';

function setup(): HTMLElement {
  render(
    <TooltipProvider>
      <Tooltip content="Settings">
        {/* Like in-app navigation, which prevents the link's default. */}
        <button type="button" onClick={(event) => { event.preventDefault(); }}>go</button>
      </Tooltip>
    </TooltipProvider>,
  );
  return screen.getByRole('button');
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('Tooltip', () => {
  it('opens after the hover delay', () => {
    fireEvent.pointerMove(setup());
    act(() => { vi.advanceTimersByTime(400); });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Settings');
  });

  it('marks its content so the stylesheet keeps it from taking the pointer', () => {
    fireEvent.pointerMove(setup());
    act(() => { vi.advanceTimersByTime(400); });
    expect(document.querySelector('[data-radix-popper-content-wrapper] > [data-tooltip]')).toHaveTextContent('Settings');
  });

  it('closes when its target is activated, even if the target prevents default', () => {
    const button = setup();
    fireEvent.pointerMove(button);
    act(() => { vi.advanceTimersByTime(400); });
    fireEvent.click(button);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('stays closed when its target is activated during the hover delay', () => {
    const button = setup();
    fireEvent.pointerMove(button);
    fireEvent.click(button);
    act(() => { vi.advanceTimersByTime(400); });
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('opens again once the pointer has left and come back', () => {
    const button = setup();
    fireEvent.pointerMove(button);
    fireEvent.click(button);
    fireEvent.pointerLeave(button);
    fireEvent.pointerMove(button);
    act(() => { vi.advanceTimersByTime(400); });
    expect(screen.getByRole('tooltip')).toHaveTextContent('Settings');
  });
});

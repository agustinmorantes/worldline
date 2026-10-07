import { act, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tanstack/react-router', () => ({
  createRootRoute: vi.fn((options: unknown) => options),
  Outlet: () => <div data-testid="route-outlet" />,
  useLocation: () => ({ pathname: '/' }),
}));

vi.mock('@/features/library/Sidebar', () => ({
  Sidebar: () => <aside data-testid="sidebar" />,
}));

vi.mock('@/app/shell/TitleBar', () => ({
  TitleBar: () => <div data-testid="titlebar" />,
}));

import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { RootLayout } from '@/app/routes/__root';

describe('RootLayout onboarding', () => {
  beforeEach(() => {
    setPref('onboardingSeen', false);
  });

  it('opens onboarding automatically on first run', () => {
    render(<RootLayout />);

    expect(screen.getByText(/your save-game time machine/i)).toBeInTheDocument();
    expect(screen.getByText(/worldline onboarding/i)).toBeInTheDocument();
    expect(getPrefs().onboardingSeen).toBe(true);
  });

  it('does not auto-open onboarding when completion flag exists', () => {
    setPref('onboardingSeen', true);
    render(<RootLayout />);

    expect(screen.queryByText(/guided setup/i)).not.toBeInTheDocument();
    expect(screen.getByTestId('route-outlet')).toBeInTheDocument();
  });

  it('reopens onboarding from the help event after first run', () => {
    setPref('onboardingSeen', true);
    render(<RootLayout />);

    act(() => {
      window.dispatchEvent(new CustomEvent('open-onboarding'));
    });

    expect(screen.getByText(/your save-game time machine/i)).toBeInTheDocument();
    expect(screen.getByText(/worldline onboarding/i)).toBeInTheDocument();
  });
});

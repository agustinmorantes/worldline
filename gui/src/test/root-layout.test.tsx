import { act, render, screen } from '@testing-library/react';
import { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tanstack/react-router', () => ({
  createRootRoute: vi.fn((options: unknown) => options),
  Outlet: () => <div data-testid="route-outlet" />,
  useLocation: () => ({ pathname: '/' }),
}));

vi.mock('@/components/Sidebar', () => ({
  Sidebar: () => <aside data-testid="sidebar" />,
}));

vi.mock('@/components/TitleBar', () => ({
  TitleBar: () => <div data-testid="titlebar" />,
}));

vi.mock('@/components/theme-provider', () => ({
  ThemeProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

import { RootLayout } from '@/routes/__root';

describe('RootLayout onboarding', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('opens onboarding automatically on first run', () => {
    render(<RootLayout />);

    expect(screen.getByText(/your save-game time machine/i)).toBeInTheDocument();
    expect(screen.getByText(/worldline onboarding/i)).toBeInTheDocument();
    expect(localStorage.getItem('worldline-onboarding-completed')).toBe('true');
  });

  it('does not auto-open onboarding when completion flag exists', () => {
    localStorage.setItem('worldline-onboarding-completed', 'true');
    render(<RootLayout />);

    expect(screen.queryByText(/guided setup/i)).not.toBeInTheDocument();
    expect(screen.getByTestId('route-outlet')).toBeInTheDocument();
  });

  it('reopens onboarding from the help event after first run', () => {
    localStorage.setItem('worldline-onboarding-completed', 'true');
    render(<RootLayout />);

    act(() => {
      window.dispatchEvent(new CustomEvent('open-onboarding'));
    });

    expect(screen.getByText(/your save-game time machine/i)).toBeInTheDocument();
    expect(screen.getByText(/worldline onboarding/i)).toBeInTheDocument();
  });
});

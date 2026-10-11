import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CaptureSection } from './CaptureSection';

vi.mock('./queries', () => ({
  saveNow: vi.fn(),
  useMonitoringSettings: vi.fn(() => ({
    data: { version_cooldown_minutes: 5, snapshot_debounce_seconds: 3, screenshots_enabled: true, sync_screenshots: false },
  })),
  useScreenshotsSupported: vi.fn(),
}));

vi.mock('@/features/library/queries', () => ({
  useSyncSettings: vi.fn(),
}));

import { useScreenshotsSupported } from './queries';
import { useSyncSettings } from '@/features/library/queries';

function setup({ supported, remote }: { supported: boolean | undefined; remote: string | null }): void {
  vi.mocked(useScreenshotsSupported).mockReturnValue({ data: supported } as unknown as ReturnType<typeof useScreenshotsSupported>);
  vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: remote } } as unknown as ReturnType<typeof useSyncSettings>);
  render(<CaptureSection />);
}

const shots = (): HTMLElement | null => screen.queryByRole('switch', { name: 'Screenshot each checkpoint' });
const syncShots = (): HTMLElement | null => screen.queryByRole('switch', { name: 'Sync screenshots' });

beforeEach(() => {
  vi.clearAllMocks();
});

describe('CaptureSection screenshots', () => {
  it('hides both switches where screenshots are not supported', () => {
    setup({ supported: false, remote: 'D:/Cloud' });
    expect(shots()).not.toBeInTheDocument();
    expect(syncShots()).not.toBeInTheDocument();
  });

  it('hides them until support is known', () => {
    setup({ supported: undefined, remote: 'D:/Cloud' });
    expect(shots()).not.toBeInTheDocument();
  });

  it('shows the switch with its note, but not the sync one while the cloud is off', () => {
    setup({ supported: true, remote: null });
    expect(shots()).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByText(/captures the game’s window/i)).toBeInTheDocument();
    expect(syncShots()).not.toBeInTheDocument();
  });

  it('shows the sync switch once the cloud is on', () => {
    setup({ supported: true, remote: 'D:/Cloud' });
    expect(syncShots()).toHaveAttribute('aria-checked', 'false');
  });
});

import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { DEFAULT_PREFS } from '@/shared/prefs/schema';
import { OverlaySection } from './OverlaySection';

vi.mock('@/features/library/queries', () => ({
  useSyncSettings: vi.fn(),
}));

import { useSyncSettings } from '@/features/library/queries';

function setup(remote: string | null = null): void {
  vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: remote } } as unknown as ReturnType<typeof useSyncSettings>);
  render(<OverlaySection />);
}

const toggle = (name: string): HTMLElement | null => screen.queryByRole('switch', { name });
const corner = (name: string): HTMLElement => screen.getByRole('radio', { name });

beforeEach(() => {
  setPref('overlay', DEFAULT_PREFS.overlay);
});

describe('OverlaySection', () => {
  it('starts with the sound, then the popup switch and its options', () => {
    setup();
    const switches = screen.getAllByRole('switch');
    expect(switches[0]).toHaveAccessibleName('Play a sound');
    expect(switches[1]).toHaveAccessibleName('Show a popup in game');
    expect(corner('Bottom right')).toHaveAttribute('aria-checked', 'true');
    expect(toggle('Quiet mode')).toBeInTheDocument();
  });

  it('turning the popup off hides its options but keeps the sound', () => {
    setup();
    fireEvent.click(screen.getByRole('switch', { name: 'Show a popup in game' }));
    expect(getPrefs().overlay.enabled).toBe(false);
    expect(toggle('Play a sound')).toBeInTheDocument();
    expect(screen.queryByRole('radiogroup')).not.toBeInTheDocument();
    expect(toggle('Quiet mode')).not.toBeInTheDocument();
  });

  it('shows the device-ahead warning only with the cloud on', () => {
    setup();
    expect(toggle('Warn when another device is ahead')).not.toBeInTheDocument();
  });

  it('shows the device-ahead warning with the cloud on', () => {
    setup('D:/Cloud');
    expect(toggle('Warn when another device is ahead')).toBeInTheDocument();
  });

  // The test `motion` mock remounts the section on every render, so focus isn't checked here.
  it('picks a corner by click, and the arrow keys move across the screen', () => {
    setup();
    fireEvent.click(corner('Top left'));
    expect(getPrefs().overlay.corner).toBe('tl');
    expect(corner('Top left')).toHaveAttribute('tabindex', '0');
    expect(corner('Bottom right')).toHaveAttribute('tabindex', '-1');

    fireEvent.keyDown(corner('Top left'), { key: 'ArrowRight' });
    expect(getPrefs().overlay.corner).toBe('tr');
    fireEvent.keyDown(corner('Top right'), { key: 'ArrowDown' });
    expect(getPrefs().overlay.corner).toBe('br');
    // Already at the right edge: stays.
    fireEvent.keyDown(corner('Bottom right'), { key: 'ArrowRight' });
    expect(getPrefs().overlay.corner).toBe('br');
    expect(screen.getByText('Bottom right', { selector: 'span' })).toBeInTheDocument();
  });

  it('writes the switches to the prefs', () => {
    setup();
    fireEvent.click(screen.getByRole('switch', { name: 'Play a sound' }));
    fireEvent.click(screen.getByRole('switch', { name: 'Quiet mode' }));
    expect(getPrefs().overlay).toMatchObject({ sound: false, quiet: true, enabled: true });
  });
});

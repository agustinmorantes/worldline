import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getPrefs, setPref } from '@/shared/prefs/prefs';
import { DEFAULT_PREFS } from '@/shared/prefs/schema';
import { OverlaySection } from './OverlaySection';

vi.mock('@/features/library/queries', () => ({
  useSyncSettings: vi.fn(),
}));
vi.mock('@/ipc/commands', () => ({
  checkShortcut: vi.fn(),
}));

import { useSyncSettings } from '@/features/library/queries';
import { checkShortcut } from '@/ipc/commands';

function setup(remote: string | null = null): void {
  vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: remote } } as unknown as ReturnType<typeof useSyncSettings>);
  render(<OverlaySection />);
}

const toggle = (name: string): HTMLElement | null => screen.queryByRole('switch', { name });
const corner = (name: string): HTMLElement => screen.getByRole('radio', { name });

const quickKeep = (): HTMLElement => screen.getByRole('button', { name: /^Quick keep hotkey: / });
const nameIt = (): HTMLElement => screen.getByRole('button', { name: /^Name it hotkey: / });
const row = (name: string): HTMLElement => screen.getByRole('group', { name });

/** A system check that answers when the test says. */
function pendingCheck(): { resolve: () => Promise<void>; reject: () => Promise<void> } {
  let settle!: { resolve: () => void; reject: (reason: unknown) => void };
  vi.mocked(checkShortcut).mockImplementationOnce(() => new Promise<void>((resolve, reject) => { settle = { resolve, reject }; }));
  return {
    resolve: () => act(async () => { settle.resolve(); await Promise.resolve(); }),
    reject: () => act(async () => { settle.reject('HotKey already registered'); await Promise.resolve(); }),
  };
}

/** Records Alt plus `letter` on the chip. */
function record(letter: string): void {
  fireEvent.click(quickKeep());
  fireEvent.keyDown(quickKeep(), { key: letter, code: `Key${letter.toUpperCase()}`, altKey: true });
}

beforeEach(() => {
  setPref('overlay', DEFAULT_PREFS.overlay);
  vi.mocked(checkShortcut).mockReset().mockResolvedValue(undefined);
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

describe('Quick keep hotkey', () => {
  it('shows its keys between the corner and quiet mode, and stays with the popup off', () => {
    setup();
    expect(quickKeep()).toHaveAccessibleName('Quick keep hotkey: Ctrl Shift K. Click to change.');
    const order = ['Corner', 'Quick keep hotkey', 'Quiet mode'].map((text) => screen.getByText(text));
    expect(order[0].compareDocumentPosition(order[1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(order[1].compareDocumentPosition(order[2]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(screen.getByRole('switch', { name: 'Show a popup in game' }));
    expect(quickKeep()).toBeInTheDocument();
    expect(within(row('Quick keep hotkey')).getByRole('button', { name: 'Turn off' })).toBeInTheDocument();
  });

  it('records a shortcut once the system has it free, ignoring modifiers alone', async () => {
    setup();
    fireEvent.click(quickKeep());
    expect(screen.getByText('Press keys…')).toBeInTheDocument();
    expect(screen.getByText('Hold a modifier and press a key. Esc cancels.')).toBeInTheDocument();
    expect(within(row('Quick keep hotkey')).queryByRole('button', { name: 'Turn off' })).not.toBeInTheDocument();

    fireEvent.keyDown(quickKeep(), { key: 'Alt', code: 'AltLeft', altKey: true });
    fireEvent.keyDown(quickKeep(), { key: 'j', code: 'KeyJ' });
    expect(screen.getByText('Press keys…')).toBeInTheDocument();
    fireEvent.keyDown(quickKeep(), { key: 'j', code: 'KeyJ', altKey: true });
    expect(checkShortcut).toHaveBeenCalledWith('Alt+J');
    await waitFor(() => { expect(getPrefs().overlay.quickKeepKey).toBe('Alt+J'); });
    expect(quickKeep()).toHaveAccessibleName('Quick keep hotkey: Alt J. Click to change.');
  });

  it('Esc and leaving the chip cancel, and keys reach it only while recording', () => {
    setup();
    fireEvent.keyDown(quickKeep(), { key: 'F9', code: 'F9' });
    expect(checkShortcut).not.toHaveBeenCalled();

    fireEvent.click(quickKeep());
    fireEvent.keyDown(quickKeep(), { key: 'Escape', code: 'Escape' });
    expect(screen.queryByText('Press keys…')).not.toBeInTheDocument();
    fireEvent.click(quickKeep());
    fireEvent.blur(quickKeep());
    expect(screen.queryByText('Press keys…')).not.toBeInTheDocument();
    expect(checkShortcut).not.toHaveBeenCalled();
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
  });

  it('turns off', () => {
    setup();
    fireEvent.click(within(row('Quick keep hotkey')).getByRole('button', { name: 'Turn off' }));
    expect(getPrefs().overlay.quickKeepKey).toBe('');
    expect(quickKeep()).toHaveAccessibleName('Quick keep hotkey: Off. Click to change.');
    expect(within(row('Quick keep hotkey')).queryByRole('button', { name: 'Turn off' })).not.toBeInTheDocument();
  });

  it('keeps the old shortcut when another app holds the new one', async () => {
    vi.mocked(checkShortcut).mockRejectedValue('HotKey already registered');
    setup();
    fireEvent.click(quickKeep());
    fireEvent.keyDown(quickKeep(), { key: 'F9', code: 'F9' });
    expect(await screen.findByRole('alert')).toHaveTextContent('Another app already uses this shortcut');
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
  });

  it('a check still pending when the hotkey is turned off doesn’t turn it back on', async () => {
    setup();
    const check = pendingCheck();
    record('j');
    expect(checkShortcut).toHaveBeenCalledWith('Alt+J');
    fireEvent.click(within(row('Quick keep hotkey')).getByRole('button', { name: 'Turn off' }));
    await check.resolve();
    expect(getPrefs().overlay.quickKeepKey).toBe('');
    expect(quickKeep()).toHaveAccessibleName('Quick keep hotkey: Off. Click to change.');
  });

  it('a check that lands later keeps the settings changed meanwhile', async () => {
    setup();
    const check = pendingCheck();
    record('j');
    fireEvent.click(screen.getByRole('switch', { name: 'Play a sound' }));
    fireEvent.click(screen.getByRole('switch', { name: 'Quiet mode' }));
    await check.resolve();
    expect(getPrefs().overlay).toMatchObject({ quickKeepKey: 'Alt+J', sound: false, quiet: true });
  });

  it('only the newest shortcut counts when the checks answer out of order', async () => {
    setup();
    const first = pendingCheck();
    record('j');
    const second = pendingCheck();
    record('l');
    await second.resolve();
    expect(getPrefs().overlay.quickKeepKey).toBe('Alt+L');
    await first.resolve();
    expect(getPrefs().overlay.quickKeepKey).toBe('Alt+L');
  });

  it('a stale check that fails shows no error', async () => {
    setup();
    const check = pendingCheck();
    record('j');
    fireEvent.click(within(row('Quick keep hotkey')).getByRole('button', { name: 'Turn off' }));
    await check.reject();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(getPrefs().overlay.quickKeepKey).toBe('');
  });

  it('a check that answers after Settings closed changes nothing', async () => {
    vi.mocked(useSyncSettings).mockReturnValue({ data: { remote_path: null } } as unknown as ReturnType<typeof useSyncSettings>);
    const { unmount } = render(<OverlaySection />);
    const check = pendingCheck();
    record('j');
    unmount();
    await check.resolve();
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
  });

  it('refuses the other hotkey’s shortcut without asking the system', () => {
    setup();
    fireEvent.click(quickKeep());
    fireEvent.keyDown(quickKeep(), { key: 'n', code: 'KeyN', ctrlKey: true, shiftKey: true });
    expect(screen.getByRole('alert')).toHaveTextContent('Already used by the other hotkey');
    expect(checkShortcut).not.toHaveBeenCalled();
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
  });
});

describe('Name it hotkey', () => {
  it('shows its keys right after Quick keep, and stays with the popup off', () => {
    setup();
    expect(nameIt()).toHaveAccessibleName('Name it hotkey: Ctrl Shift N. Click to change.');
    const order = ['Quick keep hotkey', 'Name it hotkey', 'Quiet mode'].map((text) => screen.getByText(text));
    expect(order[0].compareDocumentPosition(order[1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(order[1].compareDocumentPosition(order[2]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(screen.getByRole('switch', { name: 'Show a popup in game' }));
    expect(nameIt()).toBeInTheDocument();
  });

  it('records its own shortcut once the system has it free', async () => {
    setup();
    fireEvent.click(nameIt());
    fireEvent.keyDown(nameIt(), { key: 'm', code: 'KeyM', altKey: true });
    expect(checkShortcut).toHaveBeenCalledWith('Alt+M');
    await waitFor(() => { expect(getPrefs().overlay.nameItKey).toBe('Alt+M'); });
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
  });

  it('refuses Quick keep’s shortcut without asking the system', () => {
    setup();
    fireEvent.click(nameIt());
    fireEvent.keyDown(nameIt(), { key: 'k', code: 'KeyK', ctrlKey: true, shiftKey: true });
    expect(within(row('Name it hotkey')).getByRole('alert')).toHaveTextContent('Already used by the other hotkey');
    expect(checkShortcut).not.toHaveBeenCalled();
    expect(getPrefs().overlay.nameItKey).toBe('Ctrl+Shift+N');
  });

  it('turns off on its own', () => {
    setup();
    fireEvent.click(within(row('Name it hotkey')).getByRole('button', { name: 'Turn off' }));
    expect(getPrefs().overlay.nameItKey).toBe('');
    expect(getPrefs().overlay.quickKeepKey).toBe('Ctrl+Shift+K');
    expect(nameIt()).toHaveAccessibleName('Name it hotkey: Off. Click to change.');
  });
});

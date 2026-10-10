import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_PREFS } from '@/shared/prefs/schema';
import { setPref } from '@/shared/prefs/prefs';
import { overlayConfig, pushOverlayConfig } from './overlay-config';

vi.mock('@/ipc/commands', () => ({
  setOverlayConfig: vi.fn().mockResolvedValue(undefined),
  getPrefsPath: vi.fn(),
}));

import { setOverlayConfig } from '@/ipc/commands';

describe('overlayConfig', () => {
  it('sends the overlay prefs with the resolved motion and language', () => {
    expect(overlayConfig(DEFAULT_PREFS, false, ['es-AR'])).toEqual({
      ...DEFAULT_PREFS.overlay,
      motion: 'full',
      language: 'es',
      keptTagTemplate: 'Conservado en el juego, {time}',
    });
    expect(overlayConfig(DEFAULT_PREFS, true, ['fr'])).toMatchObject({ motion: 'reduced', language: 'en' });
    expect(overlayConfig({ ...DEFAULT_PREFS, motion: 'full', language: 'en' }, true, ['es'])).toMatchObject({ motion: 'full', language: 'en' });
    expect(overlayConfig({ ...DEFAULT_PREFS, motion: 'reduced' }, false, [])).toMatchObject({ motion: 'reduced' });
  });

  it('sends the automatic tag in the resolved language, with the time left for Rust', () => {
    expect(overlayConfig(DEFAULT_PREFS, false, ['fr'])).toMatchObject({ keptTagTemplate: 'Kept in game, {time}' });
    expect(overlayConfig({ ...DEFAULT_PREFS, language: 'es' }, false, ['en'])).toMatchObject({ keptTagTemplate: 'Conservado en el juego, {time}' });
  });
});

describe('pushOverlayConfig', () => {
  beforeEach(() => {
    vi.mocked(setOverlayConfig).mockClear();
  });

  it('pushes once on start, then on each change that reaches the overlay', () => {
    pushOverlayConfig();
    expect(setOverlayConfig).toHaveBeenCalledTimes(1);

    setPref('overlay', { ...DEFAULT_PREFS.overlay, corner: 'tl' });
    expect(setOverlayConfig).toHaveBeenLastCalledWith(expect.objectContaining({ corner: 'tl' }));
    setPref('motion', 'reduced');
    expect(setOverlayConfig).toHaveBeenLastCalledWith(expect.objectContaining({ motion: 'reduced' }));
    setPref('language', 'es');
    expect(setOverlayConfig).toHaveBeenLastCalledWith(expect.objectContaining({ language: 'es', keptTagTemplate: 'Conservado en el juego, {time}' }));
    expect(setOverlayConfig).toHaveBeenCalledTimes(4);

    // Preferences the overlay doesn't use push nothing.
    setPref('tone', 'warm');
    setPref('scanView', 'list');
    expect(setOverlayConfig).toHaveBeenCalledTimes(4);
  });
});

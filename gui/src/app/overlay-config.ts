// Pushes the in-game popup's preferences to Rust (ADR 0009): once the prefs are loaded, then
// whenever they, the motion or the language change. Rust never reads `prefs.json`.
import { error as logError } from '@tauri-apps/plugin-log';
import { setOverlayConfig, type OverlayConfig } from '@/ipc/commands';
import { resolveLocale } from '@/shared/i18n/locale';
import { getPrefs, subscribePrefs } from '@/shared/prefs/prefs';
import type { Prefs } from '@/shared/prefs/schema';

const REDUCED_MOTION = '(prefers-reduced-motion: reduce)';

/** The config for `prefs`, with the motion and language resolved the way the app resolves them. */
export function overlayConfig(prefs: Prefs, systemReduced: boolean, languages: readonly string[]): OverlayConfig {
  const reduced = prefs.motion === 'reduced' || (prefs.motion === 'system' && systemReduced);
  return {
    ...prefs.overlay,
    motion: reduced ? 'reduced' : 'full',
    language: resolveLocale(prefs.language, languages),
  };
}

export function pushOverlayConfig(): void {
  const media = window.matchMedia(REDUCED_MOTION);
  let last = '';
  const push = (): void => {
    const config = overlayConfig(getPrefs(), media.matches, navigator.languages);
    const key = JSON.stringify(config);
    if (key === last) return;
    last = key;
    setOverlayConfig(config).catch((err: unknown) => {
      void logError(`Couldn't update the in-game popup: ${String(err)}`).catch(() => undefined);
    });
  };
  push();
  subscribePrefs(push);
  media.addEventListener('change', push);
}

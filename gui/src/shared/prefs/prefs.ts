// GUI preferences in a JSON file (tauri-plugin-store), loaded once before the
// first render and then read synchronously through `usePrefs`/`getPrefs`.
import { useSyncExternalStore } from 'react';
import { load, type Store } from '@tauri-apps/plugin-store';
import { error as logError } from '@tauri-apps/plugin-log';
import { getPrefsPath } from '@/ipc/commands';
import { DEFAULT_PREFS, parsePrefs, serializePrefs, type Prefs } from './schema';

const KEY = 'prefs';
const LEGACY_ONBOARDING_KEY = 'worldline-onboarding-completed';

let current: Prefs = DEFAULT_PREFS;
let store: Store | null = null;
const listeners = new Set<() => void>();

/** Falls back to the defaults (and logs) if the file can't be read. */
export async function loadPrefs(): Promise<void> {
  try {
    store = await load(await getPrefsPath(), { autoSave: false, defaults: {} });
    const stored = await store.get(KEY);
    current = parsePrefs(stored);
    // Installs from before the prefs file kept this flag in localStorage.
    if (stored === undefined && localStorage.getItem(LEGACY_ONBOARDING_KEY) === 'true') {
      current = { ...current, onboardingSeen: true };
    }
  } catch (err) {
    void logError(`Couldn't load preferences: ${String(err)}`).catch(() => undefined);
  }
}

export function getPrefs(): Prefs {
  return current;
}

export function setPref<K extends keyof Prefs>(key: K, value: Prefs[K]): void {
  current = { ...current, [key]: value };
  for (const listener of listeners) listener();
  void persist();
}

async function persist(): Promise<void> {
  try {
    await store?.set(KEY, serializePrefs(current));
    await store?.save();
  } catch (err) {
    void logError(`Couldn't save preferences: ${String(err)}`).catch(() => undefined);
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function usePrefs(): Prefs {
  return useSyncExternalStore(subscribe, getPrefs);
}

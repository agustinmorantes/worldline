// Whether the backend is syncing right now, from the `sync-started` / `sync-complete` /
// `sync-error` events (wired in `app/events.ts`).
import { useSyncExternalStore } from 'react';

let running = false;
const listeners = new Set<() => void>();

export function setSyncRunning(value: boolean): void {
  if (running === value) return;
  running = value;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useSyncRunning(): boolean {
  return useSyncExternalStore(subscribe, () => running);
}

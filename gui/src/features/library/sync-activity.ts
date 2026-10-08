// Whether the backend is syncing right now, from the `sync-started` / `sync-complete` /
// `sync-error` events (wired in `app/events.ts`), and whether the latest sync found the
// cloud folder unreachable (those events plus the sync report query's own runs).
import { useSyncExternalStore } from 'react';
import type { Failure } from '@/ipc/bindings/Failure';

let running = false;
let unreachable = false;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

export function setSyncRunning(value: boolean): void {
  if (running === value) return;
  running = value;
  notify();
}

/** Whether `error` says the cloud folder is missing or can't be read. */
export function isFolderUnreachable(error: unknown): boolean {
  return typeof error === 'object' && error !== null && (error as Partial<Failure>).kind === 'sync_folder_unreachable';
}

/** Records how the latest sync ended: `null` for a success, else its error. */
export function setSyncOutcome(error: unknown): void {
  const value = isFolderUnreachable(error);
  if (unreachable === value) return;
  unreachable = value;
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useSyncRunning(): boolean {
  return useSyncExternalStore(subscribe, () => running);
}

/** The latest sync couldn't reach the cloud folder; cleared by the next success. */
export function useFolderUnreachable(): boolean {
  return useSyncExternalStore(subscribe, () => unreachable);
}

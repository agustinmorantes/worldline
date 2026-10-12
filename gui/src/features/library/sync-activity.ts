// Whether the backend is syncing right now, from the `sync-started` / `sync-complete` /
// `sync-error` events (wired in `app/events.ts`), and what, if anything, blocked the latest
// sync (those events plus the sync report query's own runs).
import { useSyncExternalStore } from 'react';
import type { Failure } from '@/ipc/bindings/Failure';

/** Why syncs can't go on until something outside Worldline changes: the cloud folder is out of
 *  reach, or another device wrote it with a newer Worldline. Shown in the sync card and
 *  Settings → Cloud, never as a toast. */
export type SyncBlock = 'unreachable' | 'update';

let running = false;
let blocked: SyncBlock | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

export function setSyncRunning(value: boolean): void {
  if (running === value) return;
  running = value;
  notify();
}

/** What `error` says blocks syncing, if anything. */
export function syncBlock(error: unknown): SyncBlock | null {
  const kind = typeof error === 'object' && error !== null ? (error as Partial<Failure>).kind : undefined;
  return kind === 'sync_folder_unreachable' ? 'unreachable' : kind === 'update_required' ? 'update' : null;
}

/** Records how the latest sync ended: `null` for a success, else its error. */
export function setSyncOutcome(error: unknown): void {
  const value = syncBlock(error);
  if (blocked === value) return;
  blocked = value;
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useSyncRunning(): boolean {
  return useSyncExternalStore(subscribe, () => running);
}

/** What blocked the latest sync; cleared by the next success. */
export function useSyncBlock(): SyncBlock | null {
  return useSyncExternalStore(subscribe, () => blocked);
}

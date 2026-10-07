// Typed subscriptions to the events emitted in `src-tauri/src/lib.rs`. The only
// place in the GUI that calls `listen`.
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { SyncReport } from './bindings/SyncReport';

/** Event name → payload. */
export interface Events {
  'change-detected': number;
  /** [game id, version number] */
  'version-created': [number, number];
  'detection-error': string;
  'sync-complete': SyncReport;
  'sync-error': string;
  'history-pruned': null;
  'pruning-error': string;
}

export function on<K extends keyof Events>(
  event: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  return listen<Events[K]>(event, ({ payload }) => { handler(payload); });
}

// Typed subscriptions to the events emitted in `src-tauri/src/lib.rs`. The only
// place in the GUI that calls `listen`.
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Failure } from './bindings/Failure';
import type { SyncReport } from './bindings/SyncReport';

/** Event name → payload. */
export interface Events {
  'change-detected': number;
  /** [game id, version number] */
  'version-created': [number, number];
  'detection-error': string;
  'sync-started': null;
  'sync-complete': SyncReport;
  'sync-error': Failure;
  'history-pruned': null;
  'pruning-error': Failure;
  /** A game's process started or its last one exited; the payload is the game id. */
  'game-started': number;
  'game-stopped': number;
}

export function on<K extends keyof Events>(
  event: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  return listen<Events[K]>(event, ({ payload }) => { handler(payload); });
}

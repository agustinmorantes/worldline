// Typed subscriptions to the events emitted in `src-tauri/src/lib.rs` and
// `overlay/mod.rs`, and the overlay's replies. The only place in the GUI that
// calls `listen` and `emit`.
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Corner } from '@/shared/prefs/schema';
import type { Locale } from '@/shared/i18n/locale';
import type { BranchFrom } from './bindings/BranchFrom';
import type { CaptureFailure } from './bindings/CaptureFailure';
import type { Failure } from './bindings/Failure';
import type { SyncReport } from './bindings/SyncReport';

/** Event name → payload. */
export interface Events {
  'change-detected': number;
  /** [game id, version number, the parent when the version starts a new timeline] */
  'version-created': [number, number, BranchFrom | null];
  /** [game id, version id]: the version's screenshot arrived, usually seconds after it. */
  'screenshot-attached': [number, number];
  /** [game id, version id]: a hotkey kept the version under its automatic tag. */
  'version-kept': [number, number];
  'detection-error': string;
  /** [game id, reason]: an automatic capture failed; `detection-error` carries the details. */
  'checkpoint-failed': [number, CaptureFailure];
  'sync-started': null;
  'sync-complete': SyncReport;
  'sync-error': Failure;
  'history-pruned': null;
  'pruning-error': Failure;
  /** A game's process started or its last one exited; the payload is the game id. */
  'game-started': number;
  'game-stopped': number;
  /** To the overlay webview: show a popup, or swap the one showing. */
  'overlay-show': OverlayShow;
  /** To the overlay webview: play popup `id`'s exit; `fast` when the game left the front. */
  'overlay-leave': { id: number; fast: boolean };
}

/** A popup for the overlay webview to render; Rust has decided everything else. */
export type OverlayShow = OverlayBase & OverlayDetail;

interface OverlayBase {
  /** Every show or swap gets a new id; "done" names the one that ended. */
  id: number;
  /** A popup is showing: change it in place instead of rising again. */
  swap: boolean;
  title: string;
  accentHue: number;
  corner: Corner;
  /** Only the mark; never for warnings and errors. */
  quiet: boolean;
  motion: 'full' | 'reduced';
  language: Locale;
  durationMs: number;
}

/** What the popup says, by kind. */
export type OverlayDetail =
  | { kind: 'checkpoint'; version: number }
  | { kind: 'newTimeline'; version: number; from: BranchFrom }
  /** The hotkeys' (O3, O4); `name` once it's named. */
  | { kind: 'kept'; version: number; name: string | null }
  /** `lastAt`: the game's latest checkpoint, Unix ms. */
  | { kind: 'sessionStart'; lastAt: number | null }
  | { kind: 'deviceAhead'; device: string; count: number }
  /** `version`: a keep's checkpoint, saved but then not kept; `null` when nothing was saved. */
  | { kind: 'captureFailed'; reason: CaptureFailure; version: number | null };

/** What the overlay webview tells Rust: it listens now, and a popup's exit animation ended. */
export interface OverlaySignals {
  'overlay-ready': null;
  'overlay-done': number;
}

export function signal<K extends keyof OverlaySignals>(event: K, payload: OverlaySignals[K]): Promise<void> {
  return emit(event, payload);
}

export function on<K extends keyof Events>(
  event: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  return listen<Events[K]>(event, ({ payload }) => { handler(payload); });
}

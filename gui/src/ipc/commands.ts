// Typed wrappers over the Tauri commands in `src-tauri/src/commands.rs` and
// `dialogs.rs`. The only place in the GUI that calls `invoke`. Argument names
// are camelCase; Tauri maps them to the Rust parameter names.
import { invoke } from '@tauri-apps/api/core';
import type { DeviceKind } from './bindings/DeviceKind';
import type { DeviceMetadata } from './bindings/DeviceMetadata';
import type { FullGame } from './bindings/FullGame';
import type { Game } from './bindings/Game';
import type { GameForm } from './bindings/GameForm';
import type { HistoryBundleSummary } from './bindings/HistoryBundleSummary';
import type { HistoryTransferReport } from './bindings/HistoryTransferReport';
import type { InstallScannerItem } from './bindings/InstallScannerItem';
import type { MonitoringSettings } from './bindings/MonitoringSettings';
import type { PruningPreview } from './bindings/PruningPreview';
import type { PruningRunReport } from './bindings/PruningRunReport';
import type { RetentionSettings } from './bindings/RetentionSettings';
import type { SyncReport } from './bindings/SyncReport';
import type { SyncSettings } from './bindings/SyncSettings';
import type { Version } from './bindings/Version';
import type { NamingClose } from './events';
import type { Locale } from '@/shared/i18n/locale';
import type { OverlayPrefs } from '@/shared/prefs/schema';

// Games and playthroughs

export const getGames = (): Promise<FullGame[]> => invoke('get_games');
export const getGame = (id: number): Promise<FullGame> => invoke('get_game', { id });
export const getRunningGames = (): Promise<number[]> => invoke('get_running_games');
/** [game id, due (naive UTC)] for the games whose saves changed since their current checkpoint. */
export const getUnrecordedChanges = (): Promise<[number, string][]> => invoke('get_unrecorded_changes');
/**
 * Creates or updates a game; `dirs` replaces its tracked directories when given. A new game whose
 * saves are far larger than usual fails with `too_large` unless `allowLarge`.
 */
export const saveGame = (game: GameForm, dirs: string[] | null, allowLarge = false): Promise<Game> =>
  invoke('save_game', { game, dirs, allowLarge });
/** Deletes the game everywhere: a tombstone in the remote (ADR 0006). */
export const deleteGame = (id: number): Promise<void> => invoke('delete_game', { id });
/** Stops tracking the game on this device; its history stays in the remote (ADR 0006). */
export const untrackGame = (id: number): Promise<void> => invoke('untrack_game', { id });
/** `null` goes back to the hue from the icon. */
export const setGameAccent = (gameId: number, hue: number | null): Promise<Game> =>
  invoke('set_game_accent', { gameId, hue });
export const setGameIcon = (gameId: number, sourcePath: string): Promise<string> =>
  invoke('set_game_icon', { gameId, sourcePath });

export const createSlot = (gameId: number, name: string): Promise<FullGame> =>
  invoke('create_slot', { gameId, name });
export const switchSlot = (gameId: number, slotId: number): Promise<FullGame> =>
  invoke('switch_slot', { gameId, slotId });
export const dismissLinkedSlots = (gameId: number): Promise<FullGame> =>
  invoke('dismiss_linked_slots', { gameId });
export const renameSlot = (slotId: number, name: string): Promise<FullGame> =>
  invoke('rename_slot', { slotId, name });
export const deleteSlot = (slotId: number, replacementSlotId?: number): Promise<FullGame> =>
  invoke('delete_slot', { slotId, replacementSlotId });

export const restoreVersion = (id: number): Promise<void> => invoke('restore_version', { id });
/** An empty tag clears it. */
export const tagVersion = (id: number, tag: string): Promise<Version> =>
  invoke('tag_version', { id, tag });
/** The version's screenshot file, or null when it has none. */
export const getScreenshotPath = (versionId: number): Promise<string | null> =>
  invoke('get_screenshot_path', { versionId });

// Install scanning

export const scanInstalled = (): Promise<InstallScannerItem[]> => invoke('scan_installed');
export const getScanRoots = (): Promise<string[]> => invoke('get_scan_roots');
export const addScanRoot = (path: string): Promise<void> => invoke('add_scan_root', { path });
export const removeScanRoot = (path: string): Promise<void> => invoke('remove_scan_root', { path });

// Settings

export const getMonitoringSettings = (): Promise<MonitoringSettings> =>
  invoke('get_monitoring_settings');
export const updateMonitoringSettings = (
  monitoringSettings: MonitoringSettings,
): Promise<MonitoringSettings> => invoke('update_monitoring_settings', { monitoringSettings });
/** False below Windows 11 and off Windows: screenshot settings are hidden. */
export const screenshotsSupported = (): Promise<boolean> => invoke('screenshots_supported');

export const getPruningSettings = (): Promise<RetentionSettings> => invoke('get_pruning_settings');
export const updatePruningSettings = (settings: RetentionSettings): Promise<RetentionSettings> =>
  invoke('update_pruning_settings', { settings });
/**
 * Previews `settings` before they are saved, or the saved settings when omitted; with `untag`
 * (a version UUID), as if that checkpoint had no name.
 */
export const previewPruning = (settings?: RetentionSettings, untag?: string): Promise<PruningPreview> =>
  invoke('preview_pruning', { settings: settings ?? null, untag: untag ?? null });
export const runPruning = (): Promise<PruningRunReport> => invoke('run_pruning');
export const getRetiredVersions = (): Promise<string[]> => invoke('get_retired_versions');

/** Where pre-release data was moved aside on startup, if it was. */
export const getStartupNotice = (): Promise<string | null> => invoke('get_startup_notice');
export const getRecoveredRestores = (): Promise<number> => invoke('get_recovered_restores');

/** The tray's text, in the GUI's language (ADR 0007). */
export interface TrayLabels {
  open: string;
  exit: string;
  tooltip: string;
  hidden_hint_title: string | null;
  hidden_hint_body: string;
}

export const setTrayLabels = (labels: TrayLabels): Promise<void> =>
  invoke('set_tray_labels', { labels });
/** The in-game popup's preferences, with the motion and language the GUI resolved, and the
 *  hotkeys' automatic tag in that language, `{time}` for Rust to fill in (ADR 0007). */
export interface OverlayConfig extends OverlayPrefs {
  motion: 'full' | 'reduced';
  language: Locale;
  keptTagTemplate: string;
}

export const setOverlayConfig = (config: OverlayConfig): Promise<void> =>
  invoke('set_overlay_config', { config });
/** Rejects when a hotkey can't be registered, most often because another app holds it. */
export const checkShortcut = (accelerator: string): Promise<void> =>
  invoke('check_shortcut', { accelerator });
/** What closing the app's naming card did: whether the focus went back to the game, and why the
 * name couldn't be saved, if it couldn't. */
export interface NameItClosed { handed: boolean; error: string | null }
/** The in-app naming card closed: Rust renames the checkpoint and hands the focus back to the
 * game when it should. When it did, the card just folds instead of flying to its row. */
export const nameItClosed = (session: number, how: NamingClose, name: string): Promise<NameItClosed> =>
  invoke('name_it_closed', { session, how, name });
/** Absolute path of the GUI preferences file. */
export const getPrefsPath = (): Promise<string> => invoke('get_prefs_path');

// Sync

export const getDevice = (): Promise<DeviceMetadata> => invoke('get_device');
/** `kind: null` reads as Desktop. */
export const updateDevice = (name: string, kind: DeviceKind | null): Promise<DeviceMetadata> =>
  invoke('update_device', { name, kind });
export const getSyncSettings = (): Promise<SyncSettings> => invoke('get_sync_settings');
export const updateSyncSettings = (settings: SyncSettings): Promise<SyncSettings> =>
  invoke('update_sync_settings', { settings });
export const refreshSync = (): Promise<SyncReport> => invoke('refresh_sync');
export const adoptRemoteGame = (gameUuid: string): Promise<FullGame> =>
  invoke('adopt_remote_game', { gameUuid });
export const applyRemoteVersion = (versionId: number): Promise<void> =>
  invoke('apply_remote_version', { versionId });
export const removeSyncDevice = (deviceUuid: string): Promise<void> =>
  invoke('remove_sync_device', { deviceUuid });

// History bundles

export const inspectHistoryBundle = (bundlePath: string): Promise<HistoryBundleSummary> =>
  invoke('inspect_history_bundle', { bundlePath });
export const exportGameHistory = (gameId: number, destination: string): Promise<HistoryTransferReport> =>
  invoke('export_game_history', { gameId, destination });
/** `mappings`: tracked directory UUID → destination path. */
export const importGameHistory = (
  bundlePath: string,
  mappings: Record<string, string>,
): Promise<HistoryTransferReport> => invoke('import_game_history', { bundlePath, mappings });

// Native dialogs (stubbed in the `e2e` build). Pickers resolve to null when cancelled.

export interface FileFilter {
  name: string;
  extensions: string[];
}

export const pickFolder = (title: string): Promise<string | null> => invoke('pick_folder', { title });
export const pickFile = (title: string, filters: FileFilter[]): Promise<string | null> =>
  invoke('pick_file', { title, filters });
export const pickSavePath = (title: string, fileName: string, filters: FileFilter[]): Promise<string | null> =>
  invoke('pick_save_path', { title, fileName, filters });
export const confirm = (title: string, message: string): Promise<boolean> =>
  invoke('confirm', { title, message });

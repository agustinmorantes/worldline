import { describe, expect, it } from 'vitest';
import { makeGame, makeTrackedDir } from '@/ipc/builders';
import type { HistoryBundleSummary } from '@/ipc/bindings/HistoryBundleSummary';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { dirsToMap, folderName, newScanResults, scanRevealDelay } from './add-games';

describe('scanRevealDelay', () => {
  it('keeps the opening beat, accelerates, and stays bounded for large scans', () => {
    expect(scanRevealDelay(0)).toBe(0);
    expect(scanRevealDelay(1)).toBeCloseTo(0.09, 2);
    expect(scanRevealDelay(59)).toBeLessThan(1.2);
    expect(scanRevealDelay(1000)).toBeLessThanOrEqual(1.2);
    expect(scanRevealDelay(20) - scanRevealDelay(19)).toBeLessThan(scanRevealDelay(1));
  });
});

const item = (title: string, last_modified_at: number | null = null): InstallScannerItem => ({
  title,
  dirs: [`C:/saves/${title}`],
  candidate_dirs: [],
  install_path: null,
  icon_path: null,
  last_modified_at,
  source: 'steam',
});

describe('newScanResults', () => {
  it('drops games already in the library or the cloud, ignoring case and spacing', () => {
    const library = [makeGame({ id: 1, title: 'Elden Ring' })];
    const found = newScanResults([item('elden  ring'), item('Hades'), item('Celeste')], library, ['HADES']);
    expect(found.map((f) => f.title)).toEqual(['Celeste']);
  });

  it('puts the most recently saved first, then the rest by title', () => {
    const found = newScanResults([item('Zelda'), item('Hades', 10), item('Celeste'), item('Balatro', 20)], [], []);
    expect(found.map((f) => f.title)).toEqual(['Balatro', 'Hades', 'Celeste', 'Zelda']);
  });
});

describe('folderName', () => {
  it('takes the last segment of Windows and POSIX paths', () => {
    expect(folderName('C:\\Users\\Me\\Saved Games\\Hades')).toBe('Hades');
    expect(folderName('/home/me/.local/share/Celeste/')).toBe('Celeste');
    expect(folderName('D:\\')).toBe('');
  });
});

describe('dirsToMap', () => {
  const summary = (uuids: string[]): HistoryBundleSummary => ({
    game_uuid: 'game-1',
    title: 'Hades',
    slot_count: 1,
    version_count: 3,
    tracked_dir_count: uuids.length,
    blob_count: 3,
    retired_version_count: 0,
    active_tracked_dirs: uuids.map((uuid) => ({ uuid, label: null, source_path_hint: `C:/old/${uuid}` })),
  });

  it('asks for every folder of a game that isn’t here', () => {
    expect(dirsToMap(summary(['a', 'b']), []).map((d) => d.uuid)).toEqual(['a', 'b']);
  });

  it('skips folders the local copy of the game already tracks', () => {
    const local = makeGame({
      id: 1,
      title: 'Hades',
      uuid: 'game-1',
      tracked_dirs: [makeTrackedDir({ id: 1, path: 'C:/saves/Hades', uuid: 'a' })],
    });
    expect(dirsToMap(summary(['a', 'b']), [local]).map((d) => d.uuid)).toEqual(['b']);
  });
});

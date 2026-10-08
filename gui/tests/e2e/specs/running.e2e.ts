import { spawn, type ChildProcess } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { $, browser, expect } from '@wdio/globals';

type Invoke = (cmd: string, args?: unknown) => Promise<unknown>;

function invoke<T>(cmd: string, args: unknown = {}): Promise<T> {
  return browser.execute((cmd, args) => {
    const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } }).__TAURI_INTERNALS__;
    return tauri.invoke(cmd, args);
  }, cmd, args) as Promise<T>;
}

const dataDir = process.env.WORLDLINE_E2E_DATA_DIR ?? '';
const TITLE = 'Ember Trail';

const library = () => $('aside[aria-label="Library"]');
const row = () => library().$(`a[title="${TITLE}"]`);

// A game counts as running while a process runs from its install dir, which the install
// scan found. A copy of a harmless system program stands in for the game.
describe('Running games', () => {
  let root = '';
  let game: ChildProcess | undefined;

  before(async () => {
    await $('button=Skip, I’ll explore').click();

    // A fresh cached manifest keeps the scan offline; it knows one made-up game.
    mkdirSync(path.join(dataDir, 'cache'), { recursive: true });
    writeFileSync(path.join(dataDir, 'cache', 'manifest.yaml'), `${TITLE}:\n  files:\n    <base>/saves:\n      tags: [save]\n`);
    root = mkdtempSync(path.join(os.tmpdir(), 'worldline-e2e-games-'));
    const saves = path.join(root, TITLE, 'saves');
    mkdirSync(saves, { recursive: true });
    writeFileSync(path.join(saves, 'save.dat'), 'first');

    await invoke('add_scan_root', { path: root });
    await invoke('scan_installed');
    await invoke('save_game', { game: { id: null, title: TITLE }, dirs: [saves] });
    await browser.refresh();
    await expect(row()).toBeDisplayed();
  });

  after(() => {
    game?.kill();
    rmSync(root, { recursive: true, force: true });
  });

  it('says Recording while the game runs, and stops saying it when it exits', async () => {
    const bin = path.join(root, TITLE, 'bin');
    mkdirSync(bin, { recursive: true });
    const windows = process.platform === 'win32';
    const exe = path.join(bin, windows ? 'game.exe' : 'game');
    copyFileSync(windows ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'PING.EXE') : '/bin/sleep', exe);
    game = spawn(exe, windows ? ['-n', '120', '127.0.0.1'] : ['120'], { stdio: 'ignore' });

    await row().click();
    await expect($(`h1=${TITLE}`)).toBeDisplayed();
    await expect(row().$('span=Recording')).toBeExisting({ wait: 20_000 });
    await expect($('header:has(h1)').$('span=Recording')).toBeDisplayed();

    game.kill();
    await expect(row().$('span=Recording')).not.toBeExisting({ wait: 20_000 });
    await expect($('header:has(h1)').$('span=Recording')).not.toBeExisting();
  });
});

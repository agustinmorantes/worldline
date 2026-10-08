import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
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

/** What the next native picker returns (see `dialogs.rs`). */
function answerDialog(answer: string): void {
  writeFileSync(path.join(dataDir, 'dialog-answer'), answer);
}

const library = () => $('aside[aria-label="Library"]');
const menuItem = (text: string) => $(`[role="menuitem"]*=${text}`);

async function openMenu(): Promise<void> {
  await $('button[aria-label="More options"]').click();
}

/** Syncs, then reloads so the UI starts from the fresh report. */
async function syncAndReload(): Promise<void> {
  await invoke('refresh_sync');
  await browser.refresh();
}

// One journey: add a game from its save folder, export its history, remove it from this
// PC, then bring it back by importing the bundle.
describe('Add games', () => {
  const saves = path.join(dataDir, 'saves', 'Hades');
  const bundle = path.join(dataDir, 'exports', 'Hades history');
  let remote = '';

  before(async () => {
    await $('button=Skip').click();
    await $('button=Yes, skip everything').click();

    mkdirSync(saves, { recursive: true });
    writeFileSync(path.join(saves, 'save.dat'), 'first');
    mkdirSync(path.dirname(bundle), { recursive: true });
    // The sync folder can't live inside the data directory.
    remote = mkdtempSync(path.join(os.tmpdir(), 'worldline-e2e-remote-'));
    await invoke('update_sync_settings', { settings: { remote_path: remote } });
    await syncAndReload();
  });

  after(() => { rmSync(remote, { recursive: true, force: true }); });

  it('adds a game from a save folder', async () => {
    await library().$('a*=Add games').click();
    answerDialog(saves);
    await $('button*=Pick a save folder').click();
    await expect($('input[value="Hades"]')).toBeDisplayed();
    await $('button=Add game').click();

    await expect($('h1=Hades')).toBeDisplayed();
    await expect(library().$('a[title="Hades"]')).toBeDisplayed();
  });

  it('exports the history', async () => {
    answerDialog(bundle);
    await openMenu();
    await menuItem('Export history').click();
    await browser.waitUntil(() => existsSync(bundle), { timeoutMsg: 'the bundle was not written' });
  });

  it('removes the game from this PC', async () => {
    await syncAndReload();
    await expect($('h1=Hades')).toBeDisplayed();
    await openMenu();
    const remove = menuItem('Remove from this PC');
    await expect(remove).not.toHaveAttribute('data-disabled');
    await remove.click();

    await expect(library().$('a[title="Hades"]')).not.toBeExisting();
    await library().$('a*=Add games').click();
    await expect($('section[aria-labelledby="add-cloud"]').$('span=Hades')).toBeDisplayed();
  });

  it('imports the bundle back', async () => {
    answerDialog(bundle);
    await $('button*=Open a history bundle').click();
    answerDialog(saves);
    await $('button=Choose folder').click();
    await $('button=Import').click();

    await expect($('h1=Hades')).toBeDisplayed();
    await expect(library().$('a[title="Hades"]')).toBeDisplayed();
  });
});

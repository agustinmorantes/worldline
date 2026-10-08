import { mkdtempSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
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

interface TrayLabels { open: string; exit: string }

const library = () => $('aside[aria-label="Library"]');
const section = (name: string) => $(`section[aria-labelledby]*=${name}`);

describe('Settings', () => {
  let remote = '';

  before(async () => {
    await $('button=Skip, I’ll explore').click();
    // The sync folder can't live inside the data directory.
    remote = mkdtempSync(path.join(os.tmpdir(), 'worldline-e2e-remote-'));
    await library().$('a*=Settings').click();
    await expect($('h1=Settings')).toBeDisplayed();
  });

  after(() => { rmSync(remote, { recursive: true, force: true }); });

  it('turns on the cloud with a folder and names this device', async () => {
    const cloud = section('Cloud');
    answerDialog(remote);
    await cloud.$('button=Choose folder').click();

    const name = cloud.$('input');
    await name.setValue('Test rig');
    await cloud.$('[role="radio"]*=Laptop').click();
    await cloud.$('button=Turn on cloud').click();

    await expect(cloud.$('span=Stored in')).toBeDisplayed();
    await expect(cloud.$(`span=${remote}`)).toBeDisplayed();
    await expect(cloud.$('span=Test rig')).toBeDisplayed();
    const settings = await invoke<{ remote_path: string | null }>('get_sync_settings');
    expect(settings.remote_path).toBe(remote);
  });

  it('switches the language of the window and the tray', async () => {
    await section('Appearance').$('[role="radio"]=Español').click();
    await expect($('h1=Configuración')).toBeDisplayed();
    await expect($('aside').$('a*=Configuración')).toBeDisplayed();
    await browser.waitUntil(async () => (await invoke<TrayLabels>('e2e_tray_labels')).open === 'Abrir');
    expect((await invoke<TrayLabels>('e2e_tray_labels')).exit).toBe('Salir');

    await section('Apariencia').$('[role="radio"]=English').click();
    await expect($('h1=Settings')).toBeDisplayed();
    await browser.waitUntil(async () => (await invoke<TrayLabels>('e2e_tray_labels')).open === 'Open');
  });

  it('shows an unavailable cloud folder until it is back', async () => {
    const card = library().$('button[aria-label^="Sync now"]');
    const unavailable = section('Cloud').$('span=Cloud folder not available');
    const away = `${remote}-away`;
    renameSync(remote, away);
    try {
      await card.click();
      await expect(card).toHaveText(/Cloud folder not available/);
      await expect(unavailable).toBeDisplayed();
    } finally {
      renameSync(away, remote);
    }

    await card.click();
    await expect(card).toHaveText(/Synced/);
    await expect(unavailable).not.toBeDisplayed();
  });

  it('turns the cloud off and leaves the folder alone', async () => {
    const cloud = section('Cloud');
    await cloud.$('button=Turn off cloud').click();

    await expect(cloud.$('button=Choose folder')).toBeDisplayed();
    await expect(library().$('button*=Turn on cloud')).toBeDisplayed();
    const settings = await invoke<{ remote_path: string | null }>('get_sync_settings');
    expect(settings.remote_path).toBeNull();
    expect(readdirSync(remote)).not.toHaveLength(0);
  });
});

// Screenshots on a Windows 11 machine: the settings show, and a checkpoint's screenshot
// sidecar appears in its panel when `screenshot-attached` says it arrived.
import { mkdirSync, writeFileSync } from 'node:fs';
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
const library = () => $('aside[aria-label="Library"]');
const panel = () => $('aside[aria-label="Selected checkpoint"]');
const shot = () => panel().$('img[alt="Screenshot of this checkpoint"]');

interface FullGame {
  id: number;
  folder_slug: string;
  active_slot: { folder_slug: string } | null;
  versions: { id: number; folder_slug: string }[];
}

interface Canvas {
  width: number;
  height: number;
  getContext: (id: '2d') => { fillStyle: string; fillRect: (x: number, y: number, w: number, h: number) => void } | null;
  toDataURL: (type: string, quality: number) => string;
}

/** A small JPEG, drawn by the webview itself. */
async function jpeg(): Promise<Buffer> {
  const url = await (browser.execute(() => {
    const doc = (globalThis as unknown as { document: { createElement: (tag: 'canvas') => Canvas } }).document;
    const canvas = doc.createElement('canvas');
    canvas.width = 160;
    canvas.height = 90;
    const ctx = canvas.getContext('2d');
    if (ctx === null) throw new Error('No 2D context');
    ctx.fillStyle = '#3a6ea5';
    ctx.fillRect(0, 0, 160, 90);
    return canvas.toDataURL('image/jpeg', 0.8);
  }) as unknown as Promise<string>);
  return Buffer.from(url.slice(url.indexOf(',') + 1), 'base64');
}

describe('Screenshots', () => {
  before(async function () {
    await $('button=Skip, I’ll explore').click();
    // Linux, and Windows before 11 (CI runners included), have no screenshots.
    if (!(await invoke<boolean>('screenshots_supported'))) this.skip();
  });

  it('offers the screenshot setting on Windows 11, on by default', async () => {
    await library().$('a*=Settings').click();
    await expect($('h1=Settings')).toBeDisplayed();
    const capture = $('section[aria-labelledby]*=Capture');
    await expect(capture.$('span=Screenshot each checkpoint')).toBeDisplayed();
    const sw = capture.$('button[role="switch"]');
    await expect(sw).toHaveAttribute('aria-checked', 'true');
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'false');
    await browser.waitUntil(async () => !(await invoke<{ screenshots_enabled: boolean }>('get_monitoring_settings')).screenshots_enabled, {
      timeoutMsg: 'turning it off was not saved',
    });
    await sw.click();
    await expect(sw).toHaveAttribute('aria-checked', 'true');
    await browser.waitUntil(async () => (await invoke<{ screenshots_enabled: boolean }>('get_monitoring_settings')).screenshots_enabled, {
      timeoutMsg: 'the setting was not saved',
    });
    // Syncing them is offered only with the cloud on.
    await expect(capture.$('span=Sync screenshots')).not.toBeExisting();
  });

  it('shows a checkpoint’s screenshot once it arrives', async () => {
    const saves = path.join(dataDir, 'saves', 'celeste');
    mkdirSync(saves, { recursive: true });
    writeFileSync(path.join(saves, 'save.dat'), 'first');
    const created = await invoke<{ id: number }>('save_game', { game: { id: null, title: 'Celeste' }, dirs: [saves] });
    const game = await invoke<FullGame>('get_game', { id: created.id });
    const version = game.versions.at(0);
    if (game.active_slot === null || version === undefined) throw new Error('No first checkpoint');

    await browser.refresh();
    await library().$('.//a[.//span[text()="Celeste"]]').click();
    await expect(panel().$('span=You are here')).toBeDisplayed();
    await expect(shot()).not.toBeExisting();

    // What the screenshot worker does: write the sidecar, then announce it.
    const versionDir = path.join(dataDir, 'repository', 'games', game.folder_slug, 'slots', game.active_slot.folder_slug, 'versions', version.folder_slug);
    writeFileSync(path.join(versionDir, 'screenshot.jpg'), await jpeg());
    await browser.execute((gameId, versionId) => {
      const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } }).__TAURI_INTERNALS__;
      return tauri.invoke('plugin:event|emit', { event: 'screenshot-attached', payload: [gameId, versionId] });
    }, game.id, version.id);

    await expect(shot()).toBeDisplayed();
    await expect(shot()).toHaveAttribute('loading', 'lazy');
    // Loaded through the asset protocol, inside its scope.
    const img = await shot();
    await browser.waitUntil(
      async () => (await (browser.execute((el) => (el as unknown as { naturalWidth: number }).naturalWidth, img) as unknown as Promise<number>)) === 160,
      { timeoutMsg: 'the screenshot did not load' },
    );
  });
});

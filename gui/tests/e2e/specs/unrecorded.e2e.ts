import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { $, browser, expect } from '@wdio/globals';

type Invoke = (cmd: string, args?: unknown) => Promise<unknown>;

function invoke<T>(cmd: string, args: unknown = {}): Promise<T> {
  return browser.execute((cmd, args) => {
    const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } }).__TAURI_INTERNALS__;
    return tauri.invoke(cmd, args);
  }, cmd, args) as Promise<T>;
}

// The game saves inside the cooldown: the timeline says so, and the checkpoint you are on can
// be gone back to, keeping that save as its own checkpoint first.
describe('Unrecorded changes', () => {
  const saves = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'saves', 'silksong');
  const save = path.join(saves, 'save.dat');
  let gameId = 0;

  before(async () => {
    await $('button=Skip, I’ll explore').click();

    // The first checkpoint is taken when the game is added; the long cooldown holds the next one back.
    mkdirSync(saves, { recursive: true });
    writeFileSync(save, 'before the boss');
    await invoke('update_monitoring_settings', { monitoringSettings: { version_cooldown_minutes: 30, snapshot_debounce_seconds: 1 } });
    gameId = (await invoke<{ id: number }>('save_game', { game: { id: null, title: 'Silksong' }, dirs: [saves] })).id;
    await browser.refresh();
    await $('aside[aria-label="Library"]').$('.//a[.//span[text()="Silksong"]]').click();
    await expect($('h1=Silksong')).toBeDisplayed();
  });

  it('offers to go back to the checkpoint you are on, keeping the newer save', async () => {
    const panel = $('aside[aria-label="Selected checkpoint"]');
    await expect(panel.$('span=You are here')).toBeDisplayed();

    writeFileSync(save, 'died');

    await expect(panel.$('span=Go back to')).toBeDisplayed({ wait: 20_000 });
    await expect($('[role="img"][aria-label^="Not recorded yet · recorded in ~"]')).toBeDisplayed();

    const hold = panel.$('button*=Hold to go back');
    // It just slid in, maybe below the panel's fold: WebKit's driver won't press it until it's in view.
    await hold.scrollIntoView();
    await hold.waitForClickable();
    await browser.action('pointer').move({ origin: await hold }).down().pause(1300).up().perform();

    await browser.waitUntil(() => readFileSync(save, 'utf8') === 'before the boss', { timeoutMsg: 'the save file was not restored' });
    await expect(panel.$('span=You are here')).toBeDisplayed();
    await expect($('[role="img"][aria-label^="Not recorded yet"]')).not.toBeDisplayed();
    const { versions } = await invoke<{ versions: { reason: string }[] }>('get_game', { id: gameId });
    expect(versions.map((v) => v.reason)).toContain('pre_restore');
  });
});

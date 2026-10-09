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

describe('Sessions', () => {
  const saves = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'saves', 'hades');
  const save = path.join(saves, 'save.dat');

  before(async () => {
    await $('button=Skip, I’ll explore').click();

    // Two checkpoints in a line: the first is taken when the game is added; restoring it
    // over a changed file checkpoints that change first, and restoring that comes back.
    mkdirSync(saves, { recursive: true });
    writeFileSync(save, 'first');
    const game = await invoke<{ id: number }>('save_game', { game: { id: null, title: 'Hades' }, dirs: [saves] });
    const [first] = (await invoke<{ versions: { id: number }[] }>('get_game', { id: game.id })).versions;
    writeFileSync(save, 'second');
    await invoke('restore_version', { id: first.id });
    const second = (await invoke<{ versions: { id: number }[] }>('get_game', { id: game.id })).versions.find((v) => v.id !== first.id);
    if (second === undefined) throw new Error('No checkpoint of the changed file');
    await invoke('restore_version', { id: second.id });
    expect(readFileSync(save, 'utf8')).toBe('second');

    await browser.refresh();
    await $('aside[aria-label="Library"]').$('.//a[.//span[text()="Hades"]]').click();
    await expect($('h1=Hades')).toBeDisplayed();
  });

  it('names a checkpoint and rewinds to it', async () => {
    const list = $('[aria-label="Timeline. Arrow keys move between checkpoints."]');
    const panel = $('aside[aria-label="Selected checkpoint"]');
    await expect(panel.$('span=You are here')).toBeDisplayed();

    await browser.execute((el) => { (el as unknown as { focus: () => void }).focus(); }, await list);
    await browser.keys('ArrowDown');
    await expect(panel.$('span=Rewind to')).toBeDisplayed();

    const name = panel.$('input[aria-label="Name this checkpoint"]');
    await name.setValue('Fresh start');
    await browser.keys('Enter');
    await expect(list.$('span=Fresh start')).toBeDisplayed();

    const hold = panel.$('button*=Hold to rewind');
    await browser.action('pointer').move({ origin: await hold }).down().pause(1300).up().perform();

    await browser.waitUntil(() => readFileSync(save, 'utf8') === 'first', { timeoutMsg: 'the save file was not restored' });
    await expect(panel.$('span=You are here')).toBeDisplayed();
    await expect(panel.$('div=Fresh start')).toBeDisplayed();
  });
});

import { mkdirSync, utimesSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { $, browser, expect } from '@wdio/globals';

type Invoke = (cmd: string, args?: unknown) => Promise<unknown>;
function invoke<T>(cmd: string, args: unknown = {}): Promise<T> {
  return browser.execute((cmd, args) => {
    const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } }).__TAURI_INTERNALS__;
    return tauri.invoke(cmd, args);
  }, cmd, args) as Promise<T>;
}

async function titles(): Promise<string[]> {
  const result: string[] = [];
  const rows = await browser.$$('aside[aria-label="Library"] a');
  for (const row of rows) {
    const text = await row.getText();
    const title = ['Recent game', 'Old import'].find((title) => text.includes(title));
    if (title !== undefined) result.push(title);
  }
  return result;
}

describe('Imported save activity', () => {
  it('orders by save dates and advances only when changed saves are captured', async () => {
    await $('button=Skip, I’ll explore').click();
    let oldVersion = 0;
    let oldSave = '';
    for (const [title, date] of [['Recent game', '2025-01-01'], ['Old import', '2020-01-01']]) {
      const dir = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'saves', title, 'nested');
      mkdirSync(dir, { recursive: true });
      const file = path.join(dir, 'save.dat');
      writeFileSync(file, 'existing progress');
      utimesSync(file, new Date(date), new Date(date));
      const game = await invoke<{ id: number }>('save_game', { game: { id: null, title }, dirs: [path.dirname(dir)] });
      if (title === 'Old import') {
        oldVersion = (await invoke<{ versions: { id: number }[] }>('get_game', { id: game.id })).versions[0].id;
        oldSave = file;
      }
    }
    await browser.refresh();
    await browser.waitUntil(async () => (await titles()).length === 2);
    expect(await titles()).toEqual(['Recent game', 'Old import']);
    await invoke('restore_version', { id: oldVersion });
    await browser.refresh();
    await browser.waitUntil(async () => (await titles()).length === 2);
    expect(await titles()).toEqual(['Recent game', 'Old import']);
    writeFileSync(oldSave, 'new progress');
    await invoke('restore_version', { id: oldVersion });
    await browser.refresh();
    await browser.waitUntil(async () => (await titles())[0] === 'Old import');
    expect(await titles()).toEqual(['Old import', 'Recent game']);
  });
});

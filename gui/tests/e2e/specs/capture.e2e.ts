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

// The core loop: the game saves, the watcher turns it into a checkpoint, the timeline shows it.
describe('Capture', () => {
  const saves = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'saves', 'celeste');

  before(async () => {
    await $('button=Skip, I’ll explore').click();

    // An empty save folder: no first checkpoint, so no cooldown holds the next one back.
    mkdirSync(saves, { recursive: true });
    await invoke('update_monitoring_settings', { monitoringSettings: { version_cooldown_minutes: 1, snapshot_debounce_seconds: 1 } });
    await invoke('save_game', { game: { id: null, title: 'Celeste' }, dirs: [saves] });
    await browser.refresh();
    await $('aside[aria-label="Library"]').$('a[title="Celeste"]').click();
    await expect($('h1=Celeste')).toBeDisplayed();
  });

  it('shows a checkpoint when the game saves', async () => {
    await expect($('h2=Waiting for Celeste')).toBeDisplayed();

    writeFileSync(path.join(saves, 'save.dat'), 'chapter 1');

    const panel = $('aside[aria-label="Selected checkpoint"]');
    await expect(panel.$('span=You are here')).toBeDisplayed({ wait: 20_000 });
    await expect($('[aria-label="Timeline. Arrow keys move between checkpoints."]').$('h3=Today')).toBeDisplayed();
  });
});

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

interface Game {
  id: number;
  active_slot: { id: number; name: string } | null;
  slots: { id: number; name: string }[];
  slot_summaries: { slot_id: number; version_count: number }[];
}

describe('Playthroughs', () => {
  const saves = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'saves', 'celeste');
  const save = path.join(saves, 'save.dat');
  let gameId = 0;
  let mainId = 0;

  /** The slot named `name` (a recovered one may come back under a new id) and its checkpoint count. */
  const slot = async (name: string): Promise<{ id: number; versions: number }> => {
    const game = await invoke<Game>('get_game', { id: gameId });
    const id = game.slots.find((s) => s.name === name)?.id ?? -1;
    return { id, versions: game.slot_summaries.find((s) => s.slot_id === id)?.version_count ?? -1 };
  };

  before(async () => {
    await $('button=Skip, I’ll explore').click();

    // Main run has a checkpoint and progress since; Golden has its own checkpoint.
    mkdirSync(saves, { recursive: true });
    writeFileSync(save, 'main');
    gameId = (await invoke<Game>('save_game', { game: { id: null, title: 'Celeste' }, dirs: [saves] })).id;
    const game = await invoke<Game>('get_game', { id: gameId });
    mainId = game.active_slot?.id ?? 0;
    await invoke('rename_slot', { slotId: mainId, name: 'Main run' });
    const golden = await invoke<Game>('create_slot', { gameId, name: 'Golden' });
    writeFileSync(save, 'golden');
    await invoke('switch_slot', { gameId, slotId: mainId });
    expect(readFileSync(save, 'utf8')).toBe('main');
    expect(golden.active_slot?.name).toBe('Golden');
    writeFileSync(save, 'main, unrecorded');

    await browser.refresh();
    await $('aside[aria-label="Library"]').$('.//a[.//span[text()="Celeste"]]').click();
    await expect($('h1=Celeste')).toBeDisplayed();
  });

  it('deletes the current playthrough after switching to a picked one, and Undo brings it back as it was', async () => {
    const before = (await slot('Main run')).versions;

    await $('button[aria-label="Playthrough"]').click();
    await $('[role="menuitem"]*=Manage playthroughs').click();
    const dialog = $('[role="dialog"]');
    await dialog.$('button[aria-label="Delete Main run"]').click();
    const question = dialog.$('[role="group"][aria-label="Delete Main run everywhere?"]');
    await expect(question).toHaveText(expect.stringContaining('30 days'));
    await question.$('label*=Golden').click();
    await question.$('button[data-action="confirm"]').click();

    // The saves switched to Golden through the guarded write; the outgoing progress was kept.
    await browser.waitUntil(() => readFileSync(save, 'utf8') === 'golden', { timeoutMsg: 'the saves did not switch to Golden' });
    await expect(dialog.$('button[aria-label="Delete Main run"]')).not.toBeExisting();
    const deleted = await invoke<Game>('get_game', { id: gameId });
    expect(deleted.active_slot?.name).toBe('Golden');
    expect(deleted.slots.map((s) => s.name)).toEqual(['Golden']);

    await $('[data-sonner-toast]').$('button=Undo').click();

    // Back, not current, and nothing written over the saves.
    await expect(dialog.$('button[aria-label="Delete Main run"]')).toBeDisplayed();
    const back = await invoke<Game>('get_game', { id: gameId });
    expect(back.active_slot?.name).toBe('Golden');
    expect(back.slots.map((s) => s.name).sort()).toEqual(['Golden', 'Main run']);
    expect(readFileSync(save, 'utf8')).toBe('golden');
    const recovered = await slot('Main run');
    expect(recovered.versions).toBe(before + 1);

    // Its history holds the progress made just before the delete.
    await invoke('switch_slot', { gameId, slotId: recovered.id });
    expect(readFileSync(save, 'utf8')).toBe('main, unrecorded');
  });
});

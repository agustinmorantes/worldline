import { $, browser, expect } from '@wdio/globals';

async function addGame(title: string): Promise<void> {
  await browser.execute(async (title) => {
    const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: (cmd: string, args: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    await tauri.invoke('save_game', { game: { id: null, title }, dirs: [] });
  }, title);
}

describe('Shell', () => {
  before(async () => {
    await $('button=Skip').click();
    await $('button=Yes, skip everything').click();
    await addGame('Celeste');
    await addGame('Hades');
    await browser.refresh();
  });

  it('switches between games from the sidebar', async () => {
    const library = $('aside[aria-label="Library"]');
    const row = (title: string) => library.$(`a[title="${title}"]`);

    await row('Celeste').click();
    await expect($('h1=Celeste')).toBeDisplayed();
    await expect(row('Celeste')).toHaveAttribute('aria-current', 'page');

    await row('Hades').click();
    await expect($('h1=Hades')).toBeDisplayed();
    await expect(row('Hades')).toHaveAttribute('aria-current', 'page');
    await expect(row('Celeste')).not.toHaveAttribute('aria-current');
  });
});

import { $, browser, expect } from '@wdio/globals';
import { Key } from 'webdriverio';

type Invoke = (cmd: string, args?: unknown) => Promise<unknown>;

function invoke<T>(cmd: string, args: unknown = {}): Promise<T> {
  return browser.execute((cmd, args) => {
    const tauri = (globalThis as unknown as { __TAURI_INTERNALS__: { invoke: Invoke } }).__TAURI_INTERNALS__;
    return tauri.invoke(cmd, args);
  }, cmd, args) as Promise<T>;
}

const library = () => $('aside[aria-label="Library"]');
const row = (title: string) => library().$(`.//a[.//span[text()="${title}"]]`);

/** Clicks `title`'s row with `key` held, as a person would. */
async function clickWith(key: string, title: string): Promise<void> {
  const target = await row(title);
  await browser.actions([
    browser.action('key').down(key).pause(0).pause(0).pause(0).up(key),
    browser.action('pointer').pause(0).move({ origin: target }).down().up(),
  ]);
}

const titles = ['Celeste', 'Hades', 'Ori', 'Tunic'];

/** The picked rows' titles, read from the text only screen readers get. */
async function picked(): Promise<string[]> {
  const found: string[] = [];
  for (const title of titles) {
    if (String(await row(title).getProperty('textContent')).endsWith('Selected')) found.push(title);
  }
  return found;
}

describe('Bulk actions', () => {
  before(async () => {
    await $('button=Skip, I’ll explore').click();
    for (const title of titles) {
      await invoke('save_game', { game: { id: null, title }, dirs: [] });
    }
    await browser.refresh();
    await row('Celeste').click();
    await expect($('h1=Celeste')).toBeDisplayed();
  });

  it('picks with Ctrl and Shift without opening another window or leaving the open game', async () => {
    const windows = await browser.getWindowHandles();
    const url = await browser.getUrl();

    await clickWith(Key.Ctrl, 'Hades');
    await clickWith(Key.Ctrl, 'Celeste');
    // Shift runs from the last game picked with Ctrl, in the list's order.
    await clickWith(Key.Shift, 'Tunic');
    expect(await picked()).toEqual(['Celeste', 'Hades', 'Ori', 'Tunic']);
    await clickWith(Key.Ctrl, 'Celeste');
    expect(await picked()).toEqual(['Hades', 'Ori', 'Tunic']);

    expect(await browser.getWindowHandles()).toEqual(windows);
    expect(await browser.getUrl()).toBe(url);
    await expect($('h1=Celeste')).toBeDisplayed();
  });

  it('deletes the picked games after one confirmation, and one Undo brings them all back', async () => {
    await row('Ori').click({ button: 'right' });
    await $('[role="menuitem"]=Delete 3 games everywhere').click();
    const dialog = $('[role="dialog"]');
    await expect(dialog.$('h2=Delete 3 games everywhere?')).toBeDisplayed();
    await dialog.$('button=Delete 3 games').click();

    for (const title of ['Hades', 'Ori', 'Tunic']) await expect(row(title)).not.toBeExisting();
    await expect($('h1=Celeste')).toBeDisplayed();
    const deleted = await invoke<{ title: string }[]>('recently_deleted');
    expect(deleted.map((item) => item.title).sort()).toEqual(['Hades', 'Ori', 'Tunic']);

    await $('[data-sonner-toast]').$('button=Undo').click();
    for (const title of ['Hades', 'Ori', 'Tunic']) await expect(row(title)).toBeDisplayed();
    expect(await invoke<unknown[]>('recently_deleted')).toEqual([]);
  });
});

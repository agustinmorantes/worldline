import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { $, browser, expect } from '@wdio/globals';

describe('Onboarding', () => {
  it('shows the tour on first launch and skips it with Esc', async () => {
    const intro = $('h1=Every save. Every timeline.');
    await expect(intro).toBeDisplayed();
    await $('button=Show me').click();
    await expect($('h2=Just play.')).toBeDisplayed();
    await browser.keys('Escape');
    await expect($('h2=Just play.')).not.toBeExisting();
    await expect($('h1=No games yet')).toBeDisplayed();
  });

  it('stays skipped across a reload and a relaunch', async () => {
    const prefsFile = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'prefs.json');
    await browser.waitUntil(() => existsSync(prefsFile), { timeoutMsg: `${prefsFile} was not written` });
    const stored = JSON.parse(readFileSync(prefsFile, 'utf8')) as { prefs?: { onboardingSeen?: boolean } };
    expect(stored.prefs?.onboardingSeen).toBe(true);

    await browser.refresh();
    await expect($('h1=No games yet')).toBeDisplayed();
    await expect($('h1=Every save. Every timeline.')).not.toBeExisting();

    await browser.reloadSession();
    await expect($('h1=No games yet')).toBeDisplayed();
    await expect($('h1=Every save. Every timeline.')).not.toBeExisting();
  });

  it('replays the tour from the help menu', async () => {
    await $('button[aria-label="Help and tour"]').click();
    await $('div[role="menuitem"]*=Replay the tour').click();
    await expect($('h1=Every save. Every timeline.')).toBeDisplayed();
    await $('button=Skip, I’ll explore').click();
    await expect($('h1=Every save. Every timeline.')).not.toBeExisting();
  });
});

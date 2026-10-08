import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { $, browser, expect } from '@wdio/globals';

describe('Worldline', () => {
  it('starts on an empty data directory and shows the first-run tour', async () => {
    await expect($('aside')).toBeDisplayed();
    await expect($('h1=Every save. Every timeline.')).toBeDisplayed();
  });

  it('keeps its preferences in the data directory', async () => {
    const prefsFile = path.join(process.env.WORLDLINE_E2E_DATA_DIR ?? '', 'prefs.json');
    await browser.waitUntil(() => existsSync(prefsFile), { timeoutMsg: `${prefsFile} was not written` });
    const stored = JSON.parse(readFileSync(prefsFile, 'utf8')) as { prefs?: { onboardingSeen?: boolean } };
    expect(stored.prefs?.onboardingSeen).toBe(true);
  });
});

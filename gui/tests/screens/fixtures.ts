import { test as base } from '@playwright/test';

interface ScreenOptions {
  /** Backend fixture from `src/ipc/mocks/fixtures.ts`. */
  fixture: 'default' | 'empty' | 'library' | 'syncing' | 'sessions' | 'ghost' | 'diverged' | 'addGames' | 'scanning' | 'scanned' | 'addGamesDone';
  /** Stored preferences, merged over the defaults. */
  prefs: Record<string, unknown>;
}

export const test = base.extend<ScreenOptions>({
  fixture: ['default', { option: true }],
  prefs: [{ onboardingSeen: true }, { option: true }],
  page: async ({ page, fixture, prefs }, use) => {
    await page.addInitScript(
      ({ fixture, prefs }) => {
        localStorage.setItem('worldline-mock-fixture', fixture);
        localStorage.setItem('worldline-mock-store', JSON.stringify({ prefs: { version: 1, ...prefs } }));
      },
      { fixture, prefs },
    );
    await use(page);
  },
});

export { expect } from '@playwright/test';

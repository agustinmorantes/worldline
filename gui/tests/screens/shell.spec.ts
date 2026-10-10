import { expect, test } from './fixtures';

test.describe('empty library', () => {
  test.use({ fixture: 'empty' });

  test('offers adding games and turning on cloud', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library.getByRole('link', { name: 'Add games' })).toBeVisible();
    await expect(library.getByRole('link', { name: 'Settings' })).toBeVisible();
    await expect(library.getByRole('button', { name: /turn on cloud/i })).toBeVisible();
  });
});

test.describe('several games', () => {
  test.use({ fixture: 'library' });

  test('shows statuses, the cloud badge and the sync card', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library.getByRole('link', { name: /stardew valley/i })).toBeVisible();
    await expect(library.getByText('New from Laptop')).toBeVisible();
    await expect(library.getByText('1 game in your cloud isn’t on this PC yet')).toBeAttached();
    await expect(library.getByRole('button', { name: /sync now\. synced/i })).toBeVisible();
  });

  test('switches between games', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await library.getByRole('link', { name: /elden ring/i }).click();
    await expect(page.getByRole('heading', { name: 'Elden Ring' })).toBeVisible();
    await expect(library.getByRole('link', { name: /elden ring/i })).toHaveAttribute('aria-current', 'page');

    await library.getByRole('link', { name: /hades/i }).click();
    await expect(page.getByRole('heading', { name: 'Hades' })).toBeVisible();
    await expect(library.getByRole('link', { name: /hades/i })).toHaveAttribute('aria-current', 'page');
    await expect(library.getByRole('link', { name: /elden ring/i })).not.toHaveAttribute('aria-current');
  });

  test('collapses to an icon rail below 1100px', async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 720 });
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library).toHaveCSS('width', '72px');
  });

  test('folds into the rail and back when the window crosses 1100px', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library).toHaveCSS('width', '240px');
    await page.setViewportSize({ width: 1000, height: 720 });
    await expect(library).toHaveCSS('width', '72px');
    // Folded, the labels are out of sight but still name the links.
    await expect.poll(async () => (await library.getByText('Hades', { exact: true }).boundingBox())?.width).toBeLessThanOrEqual(1);
    await expect(library.getByRole('link', { name: /hades/i })).toBeVisible();
    await page.setViewportSize({ width: 1280, height: 720 });
    await expect(library).toHaveCSS('width', '240px');
    await expect(library.getByText('Hades', { exact: true })).toHaveCSS('opacity', '1');
  });

  test('searches the library by title', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library.getByRole('link', { name: /elden ring/i })).toBeVisible();
    await library.getByRole('button', { name: 'Search the library' }).click();
    const field = library.getByRole('searchbox', { name: 'Search the library' });
    await expect(field).toBeFocused();

    await field.fill('HADES');
    await expect(library.getByRole('link', { name: /elden ring/i })).toHaveCount(0);
    await expect(library.getByRole('link', { name: /hades/i })).toBeVisible();

    await field.fill('zelda');
    await expect(library.getByText('No games match')).toBeVisible();

    await field.press('Escape');
    await expect(field).toHaveCount(0);
    await expect(library.getByRole('button', { name: 'Search the library' })).toBeFocused();
    await expect(library.getByRole('link', { name: /elden ring/i })).toBeVisible();
    await expect(library.getByText('No games match')).toHaveCount(0);
  });

  test('opens the first match on Enter', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('complementary', { name: 'Library' }).getByRole('link', { name: /elden ring/i })).toBeVisible();
    await page.keyboard.press('/');
    await page.keyboard.type('stard');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { name: 'Stardew Valley' })).toBeVisible();
  });

  test('searches from a flyout on the rail', async ({ page }) => {
    await page.setViewportSize({ width: 1000, height: 720 });
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library.getByRole('link', { name: /elden ring/i })).toBeVisible();
    await page.keyboard.press('Control+f');
    const field = library.getByRole('searchbox');
    await expect(field).toBeFocused();
    await field.fill('elden');
    await expect(library.getByRole('link', { name: /hades/i })).toHaveCount(0);
    await field.fill('zzz');
    await expect(library.getByText('No games match')).toBeVisible();
    await field.press('Escape');
    await expect(field).toHaveCount(0);
    await expect(library.getByRole('link', { name: /hades/i })).toBeVisible();
  });
});

test.describe('sync off', () => {
  test('turns on cloud from the sync card', async ({ page }) => {
    await page.goto('/');
    const library = page.getByRole('complementary', { name: 'Library' });
    await expect(library.getByRole('link', { name: /elden ring/i })).toBeVisible();
    await library.getByRole('button', { name: /turn on cloud/i }).click();
    await expect(library.getByRole('link', { name: 'Settings' })).toHaveAttribute('aria-current', 'page');
  });
});

test.describe('syncing', () => {
  test.use({ fixture: 'syncing' });

  test('shows the sync in progress', async ({ page }) => {
    await page.goto('/');
    const card = page.getByRole('button', { name: /sync now\. syncing/i });
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute('aria-busy', 'true');
    await expect(card).toHaveAttribute('data-state', 'syncing');
  });
});

test.describe('sync states', () => {
  for (const [fixture, state, status] of [
    ['library', 'synced', /synced/i],
    ['offline', 'unavailable', /cloud folder not available/i],
    ['syncFailed', 'failed', /sync failed/i],
  ] as const) {
    test.describe(state, () => {
      test.use({ fixture });

      test(`shows the ${state} state on the card and the rail`, async ({ page }) => {
        await page.goto('/');
        const card = page.getByRole('button', { name: /sync now/i });
        await expect(card).toHaveAttribute('data-state', state);
        await expect(card).toHaveAccessibleName(status);
        // On the rail the icon alone tells the states apart.
        await page.setViewportSize({ width: 1000, height: 720 });
        await expect(card).toHaveAttribute('data-state', state);
        await expect(card.locator('svg').last()).toBeVisible();
      });
    });
  }
});

test.describe('reduced motion', () => {
  test.use({ fixture: 'library', prefs: { onboardingSeen: true, motion: 'reduced' } });

  test('switches games without the fold', async ({ page }) => {
    await page.goto('/game/1');
    await expect(page.getByRole('heading', { name: 'Elden Ring' })).toBeVisible();
    await page.getByRole('complementary', { name: 'Library' }).getByRole('link', { name: /hades/i }).click();
    // No fold: the new game is up within a frame or two, well under the 300ms fold.
    await expect(page.getByRole('heading', { name: 'Hades' })).toBeVisible({ timeout: 150 });
  });
});

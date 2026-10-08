import { expect, test } from './fixtures';

test('lists the games and opens one', async ({ page }) => {
  await page.goto('/');
  const library = page.getByRole('complementary');
  await expect(library.getByText('Elden Ring')).toBeVisible();
  await expect(library.getByText('Hollow Knight')).toBeVisible();

  await library.getByText('Elden Ring').click();
  await expect(page.getByRole('heading', { name: 'Elden Ring' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: 'Selected checkpoint' }).getByText('Before Malenia')).toBeVisible();
});

test.describe('preferences', () => {
  test.use({ prefs: { onboardingSeen: true, language: 'es', tone: 'warm', motion: 'reduced' } });

  test('are applied to the document before the first render', async ({ page }) => {
    await page.goto('/');
    const html = page.locator('html');
    await expect(html).toHaveAttribute('lang', 'es');
    await expect(html).toHaveAttribute('data-tone', 'warm');
    await expect(html).toHaveAttribute('data-motion', 'reduced');
  });
});

test('a failed load shows a localized message with copyable details', async ({ page }) => {
  await page.goto('/game/99');
  const toast = page.getByRole('listitem').filter({ hasText: /couldn’t load this/i });
  await expect(toast).toBeVisible();
  await expect(toast.getByRole('button', { name: 'Copy details' })).toBeVisible();
});

test.describe('a running game', () => {
  test.use({ fixture: 'running' });

  test('says Recording in its row and header until it stops', async ({ page }) => {
    await page.goto('/game/1');
    const row = page.getByRole('complementary', { name: 'Library' }).getByRole('link', { name: /elden ring/i });
    const header = page.locator('header', { has: page.getByRole('heading', { name: 'Elden Ring' }) });
    await expect(row.getByText('Recording')).toBeVisible();
    await expect(header.getByText('Recording')).toBeVisible();
    await expect(page.getByRole('complementary', { name: 'Library' }).getByRole('link', { name: /hollow knight/i }).getByText('Recording')).toHaveCount(0);

    await page.evaluate(() => (window as unknown as { mockEmit: (event: string, payload: unknown) => Promise<void> }).mockEmit('game-stopped', 1));
    await expect(row.getByText('Recording')).toHaveCount(0);
    await expect(header.getByText('Recording')).toHaveCount(0);
    await expect(row.getByText(/ago/)).toBeVisible();
  });
});

import { expect, test } from './fixtures';

test.describe('first run', () => {
  test.use({ fixture: 'empty', prefs: {} });

  test('opens the tour and remembers it was seen', async ({ page }) => {
    await page.goto('/');
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    await expect(tour.getByRole('heading', { name: 'Every save. Every timeline.' })).toBeVisible();
    const stored = await page.evaluate(() => localStorage.getItem('worldline-mock-store'));
    expect(JSON.parse(stored ?? '{}')).toMatchObject({ prefs: { onboardingSeen: true } });
  });

  test('reaches every step by keyboard', async ({ page }) => {
    await page.goto('/');
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    await expect(tour.getByRole('button', { name: 'Show me' })).toBeFocused();

    await page.keyboard.press('Enter');
    await expect(tour.getByRole('heading', { name: 'Just play.' })).toBeVisible();
    await expect(tour.getByRole('button', { name: 'Checkpoints' })).toHaveAttribute('aria-current', 'step');
    await page.keyboard.press('ArrowRight');
    await expect(tour.getByRole('heading', { name: 'Made a bad call?' })).toBeVisible();
    await page.keyboard.press('ArrowLeft');
    await expect(tour.getByRole('heading', { name: 'Just play.' })).toBeVisible();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(tour.getByRole('heading', { name: 'Take the other path.' })).toBeVisible();
    await page.keyboard.press('ArrowRight');
    await expect(tour.getByRole('heading', { name: 'That’s a worldline.' })).toBeVisible();
    await expect(tour.getByRole('button', { name: 'Find my games' })).toBeFocused();

    // Esc closes from anywhere, no confirm; the `?` lights up and takes the focus.
    await page.keyboard.press('Escape');
    await expect(tour).toBeHidden();
    const help = page.getByRole('button', { name: 'Help and tour' });
    await expect(help).toBeFocused();
    await expect(page.getByRole('status').filter({ hasText: 'The tour lives here' })).toBeVisible();

    // `?` opens help, which jumps back into any step.
    await page.keyboard.press('?');
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitem', { name: /replay the tour/i })).toBeVisible();
    await menu.getByRole('menuitem', { name: /rewind/i }).press('Enter');
    await expect(tour.getByRole('heading', { name: 'Made a bad call?' })).toBeVisible();
  });

  test('plays the demo with the keyboard', async ({ page }) => {
    await page.goto('/');
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    const live = tour.locator('[aria-live="polite"]');
    await expect(tour.getByRole('button', { name: 'Show me' })).toBeFocused();
    await page.keyboard.press('Enter');

    await expect(tour.getByRole('button', { name: 'Play' })).toBeFocused();
    await page.keyboard.press('Space');
    await expect(live).toContainText('every save the game makes', { ignoreCase: true });
    await expect(tour.getByRole('button', { name: 'Next' })).toBeFocused({ timeout: 10_000 });
    await expect(tour.getByText('Forest road')).toBeVisible();
    await page.keyboard.press('Space');

    await expect(tour.getByRole('button', { name: 'Open the chest' })).toBeFocused();
    await page.keyboard.press('Space');
    const hold = tour.getByRole('button', { name: /hold to rewind/i });
    await expect(hold).toBeEnabled({ timeout: 5000 });
    await expect(hold).toBeFocused();
    await page.keyboard.down('Space');
    await page.waitForTimeout(1100);
    await page.keyboard.up('Space');
    await expect(tour.getByRole('button', { name: 'Next' })).toBeFocused({ timeout: 5000 });
    await expect(live).toContainText('gold and all');
    await page.keyboard.press('Space');

    await expect(tour.getByRole('button', { name: 'Walk past it' })).toBeFocused();
    await page.keyboard.press('Space');
    await expect(tour.getByRole('button', { name: 'Finish' })).toBeFocused({ timeout: 5000 });
    await expect(tour.getByText('Castle gate')).toBeVisible();
    await page.keyboard.press('Space');

    await expect(tour.getByRole('button', { name: 'Find my games' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(tour).toBeHidden();
    await expect(page).toHaveURL(/\/add\?scan=true/);
  });

  test('switches language from the intro', async ({ page }) => {
    await page.goto('/');
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    await tour.getByRole('radio', { name: 'Español' }).click();
    await expect(page.getByRole('dialog', { name: 'Recorrido por Worldline' })).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('lang', 'es');
  });

  test('skips without a confirm', async ({ page }) => {
    await page.goto('/');
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    await tour.getByRole('button', { name: 'Skip, I’ll explore' }).click();
    await expect(tour).toBeHidden();
    await expect(page.getByRole('heading', { name: 'No games yet' })).toBeVisible();
  });
});

test.describe('empty library', () => {
  test.use({ fixture: 'empty' });

  test('links the tour and finding games', async ({ page }) => {
    await page.goto('/');
    await page.getByRole('button', { name: 'Take the tour' }).click();
    const tour = page.getByRole('dialog', { name: 'Worldline tour' });
    await expect(tour.getByRole('heading', { name: 'Just play.' })).toBeVisible();
    await tour.getByRole('button', { name: 'Skip tour' }).click();
    await expect(tour).toBeHidden();

    await page.getByRole('button', { name: 'Find my games' }).click();
    await expect(page).toHaveURL(/\/add\?scan=true/);
  });
});

test.describe('help', () => {
  test('opens with F1 and replays the tour from the start', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('complementary', { name: 'Library' }).getByText('Elden Ring')).toBeVisible();
    await page.keyboard.press('F1');
    await page.getByRole('menuitem', { name: /replay the tour/i }).click();
    await expect(page.getByRole('dialog', { name: 'Worldline tour' }).getByRole('heading', { name: 'Every save. Every timeline.' })).toBeVisible();
  });
});

test.describe('second device', () => {
  test.use({ fixture: 'library', prefs: { onboardingSeen: true, hints: {} } });

  test('points out progress from another device once', async ({ page }) => {
    await page.goto('/');
    const hint = page.getByRole('status').filter({ hasText: 'Progress from Steam Deck shows up here' });
    await expect(hint).toBeVisible();
    const stored = await page.evaluate(() => localStorage.getItem('worldline-mock-store'));
    expect(JSON.parse(stored ?? '{}')).toMatchObject({ prefs: { hints: { secondDevice: true } } });

    await page.mouse.click(700, 400);
    await expect(hint).toBeHidden();
  });
});

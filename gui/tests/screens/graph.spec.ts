import { expect, test } from './fixtures';

test.describe('graph view', () => {
  test.use({ fixture: 'sessions' });

  test('lays every timeline out on one axis, names in a column', async ({ page }) => {
    await page.goto('/game/1');
    await page.getByRole('button', { name: 'Graph', exact: true }).click();
    await expect(page).toHaveURL(/view=graph/);
    const list = page.getByLabel('Timeline. Arrow keys move between checkpoints.');
    // The Sessions view's day sections leave once their dots have flown over.
    await expect(list.getByRole('region')).toHaveCount(0);
    await expect(list.getByRole('heading', { name: 'Today' })).toBeVisible();
    // Names are labels in the graph, and the other timeline's checkpoints show without unfolding.
    await expect(list.getByRole('button', { name: 'Elysium', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: /another timeline/i })).toHaveCount(0);
  });

  test('keeps the selection when toggling views', async ({ page }) => {
    await page.goto('/game/1');
    const panel = page.getByRole('complementary', { name: 'Selected checkpoint' });
    await page.getByLabel('Timeline. Arrow keys move between checkpoints.').focus();
    await page.keyboard.press('ArrowDown');
    await expect(panel.getByRole('textbox', { name: 'Name this checkpoint' })).toHaveValue('Elysium');

    await page.getByRole('button', { name: 'Graph', exact: true }).click();
    await expect(panel.getByRole('textbox', { name: 'Name this checkpoint' })).toHaveValue('Elysium');
    await page.getByRole('button', { name: 'Sessions', exact: true }).click();
    await expect(panel.getByRole('textbox', { name: 'Name this checkpoint' })).toHaveValue('Elysium');
  });

  test('selects a checkpoint from its label', async ({ page }) => {
    await page.goto('/game/1?view=graph');
    await page.getByRole('button', { name: 'Elysium', exact: true }).click();
    const panel = page.getByRole('complementary', { name: 'Selected checkpoint' });
    await expect(panel.getByRole('textbox', { name: 'Name this checkpoint' })).toHaveValue('Elysium');
    await expect(panel.getByRole('button', { name: 'Hold to rewind' })).toBeVisible();
  });
});

test.describe('graph view, diverged', () => {
  test.use({ fixture: 'diverged' });

  test('puts the other device’s timeline in its own lane', async ({ page }) => {
    await page.goto('/game/1?view=graph');
    const list = page.getByLabel('Timeline. Arrow keys move between checkpoints.');
    await expect(list.getByRole('button', { name: 'Styx', exact: true })).toBeVisible();
  });
});

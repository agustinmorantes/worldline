import { expect, test } from './fixtures';

test.describe('sessions list', () => {
  test.use({ fixture: 'sessions' });

  test('groups checkpoints into sessions under day headers', async ({ page }) => {
    await page.goto('/game/1');
    await expect(page.getByRole('heading', { name: 'Hades' })).toBeVisible();
    const list = page.getByLabel('Timeline. Arrow keys move between checkpoints.');
    await expect(list.getByRole('heading', { name: 'Today' })).toBeVisible();
    await expect(list.getByText('Elysium', { exact: true })).toBeVisible();
    await expect(list.getByTitle('Played on Steam Deck')).toBeVisible();
    await expect(page.getByRole('main').getByText('In the cloud')).toBeVisible();
    // Played time sums the gaps within sessions.
    await expect(page.getByText(/^~\dh \d\dm$/)).toBeVisible();
  });

  test('selects checkpoints with the arrow keys', async ({ page }) => {
    await page.goto('/game/1');
    const panel = page.getByRole('complementary', { name: 'Selected checkpoint' });
    await expect(panel.getByText('You are here')).toBeVisible();
    await page.getByLabel('Timeline. Arrow keys move between checkpoints.').focus();
    await page.keyboard.press('ArrowDown');
    await expect(panel.getByText('Rewind to')).toBeVisible();
    await expect(panel.getByRole('textbox', { name: 'Name this checkpoint' })).toHaveValue('Elysium');
    await expect(panel.getByRole('button', { name: 'Hold to rewind' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(panel.getByText('You are here')).toBeVisible();
  });

  test('rewinds only after a full hold, rumbling while it charges', async ({ page }) => {
    await page.goto('/game/1');
    await page.getByRole('button', { name: /^Elysium/ }).click();
    const panel = page.getByRole('complementary', { name: 'Selected checkpoint' });
    const hold = panel.getByRole('button', { name: 'Hold to rewind' });
    // An early release drains away and does nothing.
    await hold.focus();
    await page.keyboard.down('Space');
    await page.waitForTimeout(400);
    await page.keyboard.up('Space');
    await page.waitForTimeout(600);
    await expect(panel.getByText('Rewind to')).toBeVisible();
    // A full hold shakes the button near the end, and rewinds once it completes.
    await page.keyboard.down('Space');
    await page.waitForTimeout(750);
    expect(await hold.evaluate((el) => getComputedStyle(el).transform)).not.toBe('none');
    await expect(panel.getByText('Rewind to')).toBeVisible();
    await expect(panel.getByText('You are here')).toBeVisible();
    await page.keyboard.up('Space');
  });

  test('opens another timeline from its stub', async ({ page }) => {
    await page.goto('/game/1');
    const stub = page.getByRole('button', { name: /another timeline · 1 session/i });
    await expect(stub).toHaveAttribute('aria-expanded', 'false');
    await stub.click();
    await expect(stub).toHaveAttribute('aria-expanded', 'true');
    await page.getByRole('group', { name: /another timeline/i }).getByRole('button').first().click();
    await expect(page.getByRole('complementary', { name: 'Selected checkpoint' }).getByRole('button', { name: 'Hold to switch' })).toBeVisible();
  });

  test('names a checkpoint', async ({ page }) => {
    await page.goto('/game/1');
    const name = page.getByRole('textbox', { name: 'Name this checkpoint' });
    await name.fill('Meg defeated');
    await name.press('Enter');
    await expect(page.getByLabel('Timeline. Arrow keys move between checkpoints.').getByText('Meg defeated', { exact: true })).toBeVisible();
  });

  test('becomes a drawer below 1000px', async ({ page }) => {
    await page.setViewportSize({ width: 960, height: 720 });
    await page.goto('/game/1');
    const panel = page.getByRole('complementary', { name: 'Selected checkpoint' });
    await expect(panel).toBeHidden();
    await page.getByRole('button', { name: /^Elysium/ }).click();
    await expect(panel).toBeVisible();
    await panel.getByRole('button', { name: 'Close' }).click();
    await expect(panel).toBeHidden();
  });

  test('waits for the first checkpoint of a game with none', async ({ page }) => {
    await page.goto('/game/2');
    await expect(page.getByRole('heading', { name: 'Waiting for Celeste' })).toBeVisible();
    await expect(page.getByRole('complementary', { name: 'Selected checkpoint' })).toHaveCount(0);
  });
});

test.describe('progress from another device', () => {
  test.use({ fixture: 'ghost' });

  test('shows a ghost row that continues here', async ({ page }) => {
    await page.goto('/game/1');
    await expect(page.getByRole('main').getByText('New from Steam Deck')).toBeVisible();
    await expect(page.getByText('Played on Steam Deck')).toBeVisible();
    await page.getByRole('button', { name: 'Continue here' }).click();
    await expect(page.getByRole('button', { name: 'Continue here' })).toHaveCount(0);
    await expect(page.getByLabel('Timeline. Arrow keys move between checkpoints.').getByText('Styx', { exact: true })).toBeVisible();
  });

  test('warns that rewinding moves that progress aside', async ({ page }) => {
    await page.goto('/game/1');
    await page.getByRole('button', { name: /^Elysium/ }).click();
    await expect(page.getByText('Steam Deck’s newer progress moves to its own timeline')).toBeVisible();
  });
});

test.describe('diverged', () => {
  test.use({ fixture: 'diverged' });

  test('opens the other device’s timeline from the sync chip', async ({ page }) => {
    await page.goto('/game/1');
    const stub = page.getByRole('button', { name: /steam deck’s timeline/i });
    await expect(stub).toHaveAttribute('aria-expanded', 'false');
    await page.getByRole('button', { name: 'Steam Deck played too' }).click();
    await expect(stub).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByLabel('Timeline. Arrow keys move between checkpoints.').getByText('Styx', { exact: true })).toBeVisible();
  });
});

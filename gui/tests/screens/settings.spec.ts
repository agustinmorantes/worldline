import { expect, test } from './fixtures';

test.describe('capture', () => {
  test('steps the wait and the cooldown', async ({ page }) => {
    await page.goto('/settings');
    const capture = page.getByRole('region', { name: 'Capture' });
    const wait = capture.getByRole('group', { name: 'Wait after the game saves' });
    await expect(wait.getByRole('status')).toHaveText('3 s');
    await wait.getByRole('button', { name: 'More' }).click();
    await expect(wait.getByRole('status')).toHaveText('5 s');

    const cooldown = capture.getByRole('group', { name: 'At most one checkpoint every' });
    await expect(cooldown.getByRole('status')).toHaveText('5 min');
    await cooldown.getByRole('button', { name: 'Less' }).click();
    await cooldown.getByRole('button', { name: 'Less' }).click();
    await expect(cooldown.getByRole('status')).toHaveText('1 min');
    await expect(cooldown.getByRole('button', { name: 'Less' })).toBeDisabled();
  });
});

test.describe('keep', () => {
  test('previews what a cleanup frees as the tiers move, then cleans up', async ({ page }) => {
    await page.goto('/settings');
    const keep = page.getByRole('region', { name: 'Keep' });
    await expect(keep.getByRole('switch', { name: 'Clean up automatically' })).toHaveAttribute('aria-checked', 'true');
    await expect(keep.getByText('Would free about 3.3 GB')).toBeVisible();

    const weekly = keep.getByRole('group', { name: 'Then weekly up to' });
    await expect(weekly.getByRole('status')).toHaveText('3 months');
    await weekly.getByRole('button', { name: 'More' }).click();
    await expect(weekly.getByRole('status')).toHaveText('6 months');
    await expect(keep.getByText('Would free about 2.9 GB')).toBeVisible();

    // Growing the hourly tier past the daily one pushes the daily tier along.
    const daily = keep.getByRole('group', { name: 'Then daily up to' });
    const hourly = keep.getByRole('group', { name: 'Then hourly up to' });
    for (let i = 0; i < 4; i++) await hourly.getByRole('button', { name: 'More' }).click();
    await expect(hourly.getByRole('status')).toHaveText('7 days');
    await expect(daily.getByRole('status')).toHaveText('7 days');
    await hourly.getByRole('button', { name: 'More' }).click();
    await expect(hourly.getByRole('status')).toHaveText('14 days');
    await expect(daily.getByRole('status')).toHaveText('14 days');

    const forget = keep.getByRole('group', { name: 'Forget unnamed timelines after' });
    await expect(forget.getByRole('status')).toHaveText('Off');
    await forget.getByRole('button', { name: 'Less' }).click();
    await expect(forget.getByRole('status')).toHaveText('90 days');

    await keep.getByRole('switch', { name: 'Clean up automatically' }).click();
    await expect(keep.getByRole('switch', { name: 'Clean up automatically' })).toHaveAttribute('aria-checked', 'false');

    await keep.getByRole('button', { name: 'Clean up now' }).click();
    await expect(keep.getByText(/^Freed .+ · named checkpoints kept$/)).toBeVisible();
  });
});

test.describe('cloud on', () => {
  test.use({ fixture: 'settings' });

  test('shows where it is stored and the devices using it', async ({ page }) => {
    await page.goto('/settings');
    const cloud = page.getByRole('region', { name: 'Cloud' });
    await expect(cloud.getByText('D:/Cloud/Worldline')).toBeVisible();
    await expect(cloud.getByText('Windows · this device')).toBeVisible();
    await expect(cloud.getByText('Linux · seen 2 hours ago')).toBeVisible();

    await cloud.getByRole('button', { name: 'Remove Old laptop' }).click();
    await expect(cloud.getByText('Old laptop')).toHaveCount(0);
    await expect(cloud.getByText('Steam Deck')).toBeVisible();

    await cloud.getByRole('button', { name: 'Change' }).click();
    await expect(cloud.getByText('E:/OneDrive/Worldline')).toBeVisible();
  });

  test('edits this device', async ({ page }) => {
    await page.goto('/settings');
    const cloud = page.getByRole('region', { name: 'Cloud' });
    await cloud.getByRole('button', { name: 'Edit this device' }).click();
    await cloud.getByRole('textbox', { name: 'Name' }).fill('Living room PC');
    await cloud.getByRole('radio', { name: 'Laptop' }).click();
    await cloud.getByRole('button', { name: 'Save' }).click();
    await expect(cloud.getByText('Living room PC')).toBeVisible();
  });

  test('turns the cloud off after a confirm', async ({ page }) => {
    await page.goto('/settings');
    const cloud = page.getByRole('region', { name: 'Cloud' });
    const sidebar = page.getByRole('complementary', { name: 'Library' });
    await expect(sidebar.getByRole('button', { name: /sync now/i })).toBeVisible();

    await cloud.getByRole('button', { name: 'Turn off cloud' }).click();
    await expect(cloud.getByText('Pick a folder your cloud app syncs')).toBeVisible();
    await expect(sidebar.getByRole('button', { name: /Turn on cloud/ })).toBeVisible();
    await expect(sidebar.getByRole('button', { name: /sync now/i })).toHaveCount(0);
  });
});

test.describe('cloud folder unavailable', () => {
  test.use({ fixture: 'offline' });

  test('says so in the sync card and in settings, without a toast', async ({ page }) => {
    await page.goto('/settings');
    const card = page.getByRole('complementary', { name: 'Library' }).getByRole('button', { name: /sync now/i });
    await expect(card).toContainText('Cloud folder not available');
    const cloud = page.getByRole('region', { name: 'Cloud' });
    await expect(cloud.getByRole('status')).toContainText('Cloud folder not available');
    await expect(cloud.getByRole('status')).toContainText('Check that your cloud app is running');
    // Counted once, not with a retrying assertion: toasts vanish on their own within its timeout.
    await page.waitForTimeout(500);
    expect(await page.locator('[data-sonner-toast]').count()).toBe(0);

    await card.click();
    await expect(card).toContainText('Cloud folder not available');
    await cloud.getByRole('button', { name: 'Sync now' }).click();
    await expect(cloud.getByRole('status')).toBeVisible();
    await page.waitForTimeout(500);
    expect(await page.locator('[data-sonner-toast]').count()).toBe(0);
  });
});

test.describe('cloud off', () => {
  test.use({ fixture: 'cloudOff' });

  test('turns on the cloud: choose the folder, then name this device', async ({ page }) => {
    await page.goto('/settings');
    const cloud = page.getByRole('region', { name: 'Cloud' });
    await expect(cloud.getByText('Pick a folder your cloud app syncs')).toBeVisible();
    await cloud.getByRole('button', { name: 'Choose folder' }).click();

    await expect(cloud.getByText('D:/Dropbox/Worldline')).toBeVisible();
    await expect(cloud.getByRole('textbox', { name: 'Name' })).toHaveValue('Desktop');
    await cloud.getByRole('textbox', { name: 'Name' }).fill('Deck');
    await cloud.getByRole('radio', { name: 'Handheld' }).click();
    await expect(cloud.getByRole('radio', { name: 'Handheld' })).toHaveAttribute('aria-checked', 'true');
    await cloud.getByRole('button', { name: 'Turn on cloud' }).click();

    await expect(cloud.getByText('Stored in')).toBeVisible();
    await expect(cloud.getByText('Deck', { exact: true })).toBeVisible();
    await expect(cloud.getByText('Windows · this device')).toBeVisible();
    await expect(page.getByRole('complementary', { name: 'Library' }).getByRole('button', { name: /sync now/i })).toBeVisible();
  });
});

test.describe('appearance', () => {
  test('switches the language, the background tone and motion', async ({ page }) => {
    await page.goto('/settings');
    const appearance = page.getByRole('region', { name: 'Appearance' });

    await appearance.getByRole('radio', { name: 'Black' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-tone', 'black');
    await appearance.getByRole('radio', { name: 'Reduced' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduced');

    await appearance.getByRole('radio', { name: 'Español' }).click();
    await expect(page.getByRole('heading', { name: 'Configuración' })).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('lang', 'es');
    await expect(page.getByRole('region', { name: 'Apariencia' }).getByRole('radio', { name: 'Español' })).toHaveAttribute('aria-checked', 'true');
  });
});

import { expect, test } from './fixtures';

test.describe('cloud list', () => {
  test.use({ fixture: 'addGames' });

  test('shows the cloud games not on this PC, and adding one moves it to the library', async ({ page }) => {
    await page.goto('/add');
    const cloud = page.getByRole('region', { name: 'In your cloud' });
    await expect(cloud.getByText('Hollow Knight: Silksong')).toBeVisible();
    await expect(cloud.getByText('Balatro')).toBeVisible();
    await expect(cloud.getByText('Installed here', { exact: true })).toBeVisible();
    // Already adopted, so not offered again.
    await expect(cloud.getByText('Tunic')).toHaveCount(0);

    await cloud.getByRole('button', { name: 'Add Balatro' }).click();
    await expect(cloud.getByText('Balatro')).toHaveCount(0);
    await expect(page.getByRole('complementary').getByText('Balatro')).toBeVisible();
  });
});

test.describe('scanning', () => {
  test.use({ fixture: 'scanning' });

  test('shows the scan running once asked to look', async ({ page }) => {
    await page.goto('/add');
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    await expect(page.getByRole('status')).toHaveText('Looking in Steam, Epic and your save folders…');
  });
});

test.describe('scan results', () => {
  test.use({ fixture: 'scanned' });

  test('offers the new games preselected and adds the chosen ones', async ({ page }) => {
    await page.goto('/add');
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    const scan = page.getByRole('region', { name: 'On this PC' });
    // Hades is already in the library.
    await expect(scan.getByRole('button', { name: /Hades/ })).toHaveCount(0);
    await expect(scan.getByRole('button', { name: /Sea of Stars/ })).toHaveAttribute('aria-pressed', 'true');

    await scan.getByRole('button', { name: /Outer Wilds/ }).click();
    await scan.getByRole('button', { name: 'Add 2 games' }).click();
    await expect(scan.getByRole('button', { name: 'All added' })).toBeDisabled();
    const library = page.getByRole('complementary');
    await expect(library.getByText('Sea of Stars')).toBeVisible();
    await expect(library.getByText('Cassette Beasts')).toBeVisible();
    await expect(library.getByText('Outer Wilds')).toHaveCount(0);
  });

  test('manages the folders the scan looks in', async ({ page }) => {
    await page.goto('/add');
    const scan = page.getByRole('region', { name: 'On this PC' });
    await expect(scan.getByText('D:/Games')).toBeVisible();
    await scan.getByRole('button', { name: 'Stop looking in D:/Games' }).click();
    await expect(scan.getByText('D:/Games')).toHaveCount(0);
    await expect(scan.getByText('E:/Emulation/Saves')).toBeVisible();
  });
});

test.describe('empty', () => {
  test.use({ fixture: 'addGamesDone' });

  test('says when the whole cloud is already here', async ({ page }) => {
    await page.goto('/add');
    await expect(page.getByText('Everything in your cloud is in your library.')).toBeVisible();
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    await expect(page.getByText(/nothing new/i)).toBeVisible();
  });
});

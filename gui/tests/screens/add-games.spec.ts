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

  test('filters without losing hidden selections and selects only visible games', async ({ page }) => {
    await page.goto('/add');
    const scan = page.getByRole('region', { name: 'On this PC' });
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    const all = scan.getByRole('checkbox', { name: 'Select all' });
    await all.uncheck();
    const search = scan.getByRole('searchbox');
    await search.fill('sea');
    await search.press('Control+f');
    await expect(page.getByRole('complementary').getByRole('searchbox')).toHaveCount(0);
    await all.check();
    await expect(scan.getByRole('button', { name: 'Add 1 game' })).toBeEnabled();
    await search.fill('outer');
    await expect(all).not.toBeChecked();
    await scan.getByRole('button', { name: /Outer Wilds/ }).click();
    await search.fill('');
    await expect(all).toHaveJSProperty('indeterminate', true);
    await search.fill('no such game');
    await expect(scan.getByRole('status')).toHaveText('No games match');
    await expect(all).toBeDisabled();
    await scan.getByRole('button', { name: 'Add 2 games' }).click();
    await expect(page.getByRole('complementary').getByText('Sea of Stars')).toBeVisible();
    await expect(page.getByRole('complementary').getByText('Outer Wilds')).toBeVisible();
  });

  test('morphs into rows and persists the view without losing selections', async ({ page }) => {
    await page.goto('/add');
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    const scan = page.getByRole('region', { name: 'On this PC' });
    await scan.getByRole('button', { name: /Outer Wilds/ }).click();
    await scan.getByRole('button', { name: 'List view', exact: true }).click();
    await expect(scan.getByRole('button', { name: /Outer Wilds/ })).toHaveAttribute('aria-pressed', 'false');
    await expect(scan.getByRole('button', { name: /Sea of Stars/ })).toHaveCSS('height', '52px');
    await expect.poll(() => page.evaluate(() => {
      const store = JSON.parse(localStorage.getItem('worldline-mock-store') ?? '{}') as { prefs: { scanView: string } };
      return store.prefs.scanView;
    })).toBe('list');
    // A new page shares the stored preference without replaying the fixture's page init script.
    const reopened = await page.context().newPage();
    await reopened.goto('/add');
    await reopened.getByRole('button', { name: 'Look for installed games' }).click();
    await expect(reopened.getByRole('button', { name: 'List view', exact: true })).toHaveAttribute('aria-pressed', 'true');
    await reopened.close();
  });
});

test.describe('large scan', () => {
  test.use({ fixture: 'scannedMany' });

  test('keeps toolbar and actions reachable in both views with one scroll area', async ({ page }) => {
    await page.goto('/add');
    await page.getByRole('button', { name: 'Look for installed games' }).click();
    const scan = page.getByRole('region', { name: 'On this PC' });
    const scroll = page.locator('[data-add-scroll]');
    const add = scan.getByRole('button', { name: 'Add 60 games' });
    await expect(scan.getByRole('button', { name: /^Game / })).toHaveCount(60);
    const inScrollport = async (): Promise<boolean> => {
      const box = await add.boundingBox();
      const root = await scroll.boundingBox();
      return box !== null && root !== null && box.y >= root.y && box.y + box.height <= root.y + root.height;
    };
    await expect.poll(inScrollport).toBe(true);
    await scroll.evaluate((el) => { el.scrollTop = el.scrollHeight / 2; });
    await expect.poll(inScrollport).toBe(true);
    await expect(scan.getByRole('searchbox')).toBeInViewport();
    await scroll.evaluate((el) => { el.scrollTop = 0; });
    await scan.getByRole('button', { name: 'List view', exact: true }).click();
    await expect.poll(inScrollport).toBe(true);
    await scroll.evaluate((el) => { el.scrollTop = el.scrollHeight - el.clientHeight - 100; });
    await expect.poll(inScrollport).toBe(true);
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

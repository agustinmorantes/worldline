// Captures the README's screenshots and clips from the mocked UI. Start it with
// `bunx vite --mode mock --port 1431`, then: node scripts/readme-media.mjs [base url] [out dir] [names]
// Clips are WebM; scripts/readme-gif.sh turns them into the GIFs under docs/media/.
import { chromium } from '@playwright/test';
import { rename } from 'node:fs/promises';

const [base = 'http://localhost:1431', out = '../docs/media', only] = process.argv.slice(2);
const SIZE = { width: 1280, height: 800 };

// Playwright's video has no pointer; draw one so the clips show what was clicked.
function cursor() {
  addEventListener('DOMContentLoaded', () => {
    const dot = document.createElement('div');
    dot.style.cssText = 'position:fixed;z-index:2147483647;left:-40px;top:-40px;width:18px;height:18px;margin:-9px 0 0 -9px;border-radius:50%;background:rgba(255,255,255,.85);box-shadow:0 0 0 3px rgba(0,0,0,.35);pointer-events:none;transition:transform .12s';
    document.body.append(dot);
    addEventListener('mousemove', (e) => { dot.style.left = `${e.clientX}px`; dot.style.top = `${e.clientY}px`; }, true);
    addEventListener('mousedown', () => { dot.style.transform = 'scale(.7)'; }, true);
    addEventListener('mouseup', () => { dot.style.transform = ''; }, true);
  });
}

async function open(browser, fixture, video = false) {
  const context = await browser.newContext({
    viewport: SIZE, deviceScaleFactor: video ? 1 : 1.5, colorScheme: 'dark',
    ...(video ? { recordVideo: { dir: out, size: SIZE } } : {}),
  });
  await context.addInitScript((fixture) => {
    localStorage.setItem('worldline-mock-fixture', fixture);
    localStorage.setItem('worldline-mock-store', JSON.stringify({ prefs: { version: 1, onboardingSeen: true, hints: { secondDevice: true } } }));
  }, fixture);
  if (video) await context.addInitScript(cursor);
  return [context, await context.newPage()];
}

async function shot(browser, name, fixture, path, prepare) {
  const [context, page] = await open(browser, fixture);
  await page.goto(base + path);
  if (prepare !== undefined) await prepare(page);
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${out}/${name}.png` });
  await context.close();
}

async function clip(browser, name, fixture, path, play) {
  const [context, page] = await open(browser, fixture, true);
  await page.goto(base + path);
  await page.mouse.move(900, 700);
  await page.waitForTimeout(1000);
  await play(page);
  await page.waitForTimeout(1200);
  const video = page.video();
  await context.close();
  await rename(await video.path(), `${out}/${name}.webm`);
}

async function moveTo(page, locator) {
  const box = await locator.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 25 });
  await page.waitForTimeout(250);
}

const jobs = {
  sessions: (b) => shot(b, 'sessions', 'showcase', '/game/1'),
  graph: (b) => shot(b, 'graph', 'showcase', '/game/1?view=graph'),
  sync: (b) => shot(b, 'sync', 'diverged', '/game/1'),
  'add-games': (b) => shot(b, 'add-games', 'scanned', '/add', async (p) => {
    await p.getByRole('button', { name: 'Look for installed games' }).click();
    await p.waitForTimeout(800);
    await p.mouse.move(1000, 400);
    await p.mouse.wheel(0, -2000);
  }),
  settings: (b) => shot(b, 'settings', 'settings', '/settings'),
  rewind: (b) => clip(b, 'rewind', 'showcase', '/game/1', async (page) => {
    const target = page.getByRole('button', { name: /^Before Hydra/ });
    await moveTo(page, target);
    await target.click();
    await page.waitForTimeout(900);
    const hold = page.getByRole('complementary', { name: 'Selected checkpoint' }).getByRole('button', { name: 'Hold to rewind' });
    await moveTo(page, hold);
    await page.mouse.down();
    await page.waitForTimeout(1300);
    await page.mouse.up();
    await page.waitForTimeout(1500);
  }),
  views: (b) => clip(b, 'views', 'showcase', '/game/1', async (page) => {
    const graph = page.getByRole('button', { name: 'Graph', exact: true });
    await moveTo(page, graph);
    await graph.click();
    await page.waitForTimeout(2200);
    const sessions = page.getByRole('button', { name: 'Sessions', exact: true });
    await moveTo(page, sessions);
    await sessions.click();
    await page.waitForTimeout(1500);
  }),
};

const browser = await chromium.launch();
for (const [name, job] of Object.entries(jobs)) if (only === undefined || only.split(',').includes(name)) await job(browser);
await browser.close();

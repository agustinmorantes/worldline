import { defineConfig, devices } from '@playwright/test';

// Screen tests: the UI on plain `vite dev` with the Tauri backend mocked
// (`src/ipc/mocks`). The real-app smoke test lives in `tests/e2e` (WebdriverIO).
const PORT = 1430;

export default defineConfig({
  testDir: './tests/screens',
  timeout: 30 * 1000,
  expect: { timeout: 5000 },
  fullyParallel: true,
  forbidOnly: process.env.CI !== undefined,
  retries: process.env.CI !== undefined ? 2 : 0,
  reporter: process.env.CI !== undefined ? 'list' : 'html',
  use: {
    baseURL: `http://localhost:${PORT.toString()}`,
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: `bunx vite --mode mock --port ${PORT.toString()} --strictPort`,
    url: `http://localhost:${PORT.toString()}`,
    reuseExistingServer: process.env.CI === undefined,
  },
});

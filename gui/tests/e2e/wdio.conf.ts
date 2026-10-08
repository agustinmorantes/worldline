// End-to-end tests against the real app: a debug build with the `e2e` feature
// (`bun run e2e:build`), driven through tauri-driver. The app gets a throwaway data
// directory and stubbed native dialogs.
//
// Needs `tauri-driver` (`cargo install tauri-driver --locked`) and, on Windows, an
// msedgedriver matching the installed WebView2 (on PATH, or `TAURI_NATIVE_DRIVER`).
// On Linux, WebKitWebDriver (`webkit2gtk-driver`).
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = process.platform === 'win32' ? '.exe' : '';
const application = path.resolve(here, '../../../target/debug', `worldline${exe}`);

let tauriDriver: ChildProcess | undefined;
let dataDir: string | undefined;

export const config: WebdriverIO.Config = {
  runner: 'local',
  hostname: '127.0.0.1',
  port: 4444,
  specs: ['./specs/**/*.e2e.ts'],
  maxInstances: 1,
  capabilities: [
    {
      'tauri:options': { application },
    } as WebdriverIO.Capabilities,
  ],
  logLevel: 'warn',
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: { ui: 'bdd', timeout: 60_000 },

  // Session hooks run in the worker, which is also where the specs read the env.
  beforeSession: () => {
    dataDir = mkdtempSync(path.join(os.tmpdir(), 'worldline-e2e-'));
    process.env.WORLDLINE_E2E_DATA_DIR = dataDir;
    const driver = process.env.TAURI_DRIVER ?? path.join(os.homedir(), '.cargo', 'bin', `tauri-driver${exe}`);
    const native = process.env.TAURI_NATIVE_DRIVER;
    tauriDriver = spawn(driver, native !== undefined ? ['--native-driver', native] : [], {
      stdio: [null, process.stdout, process.stderr],
    });
  },

  afterSession: () => {
    tauriDriver?.kill();
    if (dataDir !== undefined) rmSync(dataDir, { recursive: true, force: true });
  },
};

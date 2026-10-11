// Builds worldline-cli and puts it where tauri.bundle.conf.json's externalBin expects it, so the
// installers ship it next to the app. `bun run bundle` runs it. The externalBin lives in that
// separate config because tauri-build fails every plain cargo build when the binary is missing.
import { copyFileSync, mkdirSync } from 'fs';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';

const root = fileURLToPath(new URL('../..', import.meta.url));
const binaries = fileURLToPath(new URL('../src-tauri/binaries/', import.meta.url));

const host = execSync('rustc -vV', { encoding: 'utf8' }).match(/^host: (\S+)$/m)[1];
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const release = process.env.TAURI_ENV_DEBUG !== 'true';
const exe = target.includes('windows') ? '.exe' : '';

// Building for the host without --target shares the build cache with the app.
const targetArg = target === host ? '' : ` --target ${target}`;
execSync(`cargo build -p cli${release ? ' --release' : ''}${targetArg}`, { cwd: root, stdio: 'inherit' });

const outDir = `${root}target/${target === host ? '' : `${target}/`}${release ? 'release' : 'debug'}`;
mkdirSync(binaries, { recursive: true });
copyFileSync(`${outDir}/worldline-cli${exe}`, `${binaries}worldline-cli-${target}${exe}`);

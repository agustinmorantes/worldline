import { readFileSync, writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';
import { Resvg } from '@resvg/resvg-js';

const svgPath = fileURLToPath(new URL('../src-tauri/icons/source.svg', import.meta.url));
const pngPath = fileURLToPath(new URL('../src-tauri/icons/source-1024.png', import.meta.url));
const guiDir = fileURLToPath(new URL('..', import.meta.url));

const svg = readFileSync(svgPath);
const resvg = new Resvg(svg, { fitTo: { mode: 'width', value: 1024 } });
const png = resvg.render().asPng();
writeFileSync(pngPath, png);
console.log('Generated source-1024.png');

execSync(`bun tauri icon "${pngPath}"`, { stdio: 'inherit', cwd: guiDir });
console.log('Done — icons generated in src-tauri/icons/');

// Builds the app and tray icons from the small icon design, frozen at its default hue, then lets `tauri icon` fan it out.
// The main design (with the memory card) turns to mud at icon sizes, so it is not used here.
import { writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { execSync } from 'child_process';
import { Resvg } from '@resvg/resvg-js';

const iconsDir = fileURLToPath(new URL('../src-tauri/icons/', import.meta.url));
const guiDir = fileURLToPath(new URL('..', import.meta.url));
const H = 290;

// oklch (L in 0..1, C, hue in degrees) to a hex colour.
function oklch(L, C, h) {
  const a = C * Math.cos((h * Math.PI) / 180);
  const b = C * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const lin = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
  const hex = lin.map((v) => {
    const c = Math.min(1, Math.max(0, v));
    const g = c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
    return Math.round(g * 255).toString(16).padStart(2, '0');
  });
  return `#${hex.join('')}`;
}

// The split line alone on a dark tile, drawn on a 64 grid.
function smallSvg(px, sw) {
  const on = oklch(0.8, 0.14, H);
  const ring = oklch(0.21, 0.04, H);
  const r = (px * 0.225).toFixed(1);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 ${px} ${px}">
<defs><linearGradient id="t" x1="0" y1="0" x2="0.35" y2="1"><stop offset="0" stop-color="${oklch(0.28, 0.06, H)}"/><stop offset="1" stop-color="${oklch(0.16, 0.03, H - 25)}"/></linearGradient></defs>
<rect width="${px}" height="${px}" rx="${r}" fill="url(#t)"/>
<g transform="scale(${px / 64})" fill="none" stroke="${on}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round">
<path d="M10 32 L26 32"/><path d="M26 32 C 36 32 40 18 52 18"/><path d="M26 32 C 36 32 40 46 52 46"/>
<circle cx="52" cy="18" r="3.4" fill="${on}" stroke="none"/><circle cx="52" cy="46" r="3.4" fill="${on}" stroke="none"/>
<circle cx="26" cy="32" r="6" fill="${on}" stroke="${ring}" stroke-width="${sw * 0.75}"/>
</g></svg>`;
}

const render = (svg, size) => new Resvg(svg, { fitTo: { mode: 'width', value: size } }).render().asPng();

const SW = { 16: 6, 24: 5.4, 32: 5, 48: 4.4, 64: 4, 256: 4 };
const small = (px) => render(smallSvg(px, SW[px] ?? 4), px);

const source = smallSvg(1024, 4);
writeFileSync(`${iconsDir}source.svg`, source);
writeFileSync(`${iconsDir}source-1024.png`, render(source, 1024));
console.log('Generated source.svg and source-1024.png');

execSync(`bun tauri icon "${iconsDir}source-1024.png"`, { stdio: 'inherit', cwd: guiDir });

// Stroke widths per size come from the design (on its 64 grid).
writeFileSync(`${iconsDir}32x32.png`, small(32));

// The Windows icon. PNG entries, so the ICO
// is a header, a directory and the images back to back.
const entries = [16, 24, 32, 48, 64, 256].map((px) => [px, small(px)]);
const head = Buffer.alloc(6 + 16 * entries.length);
head.writeUInt16LE(1, 2);
head.writeUInt16LE(entries.length, 4);
let offset = head.length;
entries.forEach(([px, png], i) => {
  const o = 6 + 16 * i;
  head[o] = px % 256;
  head[o + 1] = px % 256;
  head.writeUInt16LE(1, o + 4);
  head.writeUInt16LE(32, o + 6);
  head.writeUInt32LE(png.length, o + 8);
  head.writeUInt32LE(offset, o + 12);
  offset += png.length;
});
writeFileSync(`${iconsDir}icon.ico`, Buffer.concat([head, ...entries.map(([, png]) => png)]));

// The tray shows it at 16 to 32 px: drawn at 32 with the 16 px stroke, so a 2x downscale
// lands on the design's 16 px weight and 200% scaling stays crisp.
writeFileSync(`${iconsDir}tray.png`, render(smallSvg(32, SW[16]), 32));
console.log('Done: icons generated in src-tauri/icons/');

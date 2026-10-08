// Builds the app and tray icons from the two designs in docs/design/mocks
// (icon-main.dc.html, icon-small.dc.html), frozen at their default hue and
// without the animations, then lets `tauri icon` fan the main one out.
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

const a1 = oklch(0.74, 0.19, H);
const a2 = oklch(0.78, 0.17, H + 50);
const hot = oklch(0.97, 0.03, H);

function mainSvg() {
  const tube = (d) =>
    `<path d="${d}" fill="none" stroke="#fff" stroke-opacity=".1" stroke-width="20" stroke-linecap="round"/>` +
    `<path d="${d}" fill="none" stroke="url(#tube)" stroke-width="8" stroke-linecap="round" filter="url(#glow)"/>`;
  const end = (y) =>
    `<circle cx="334" cy="${y}" r="12" fill="${oklch(0.2, 0.03, H)}" stroke="#fff" stroke-opacity=".35" stroke-width="2.5"/>` +
    `<circle cx="334" cy="${y}" r="5" fill="${a2}" filter="url(#glow)"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
<defs>
<linearGradient id="tile" x1="0" y1="0" x2="0.4" y2="1"><stop offset="0" stop-color="${oklch(0.31, 0.08, H)}"/><stop offset="1" stop-color="${oklch(0.13, 0.035, H - 25)}"/></linearGradient>
<radialGradient id="aura" cx="242" cy="214" r="180" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${oklch(0.7, 0.2, H)}" stop-opacity=".55"/><stop offset="1" stop-color="${oklch(0.7, 0.2, H)}" stop-opacity="0"/></radialGradient>
<linearGradient id="face" x1="0" y1="0" x2="0.3" y2="1"><stop offset="0" stop-color="${oklch(0.29, 0.035, H)}"/><stop offset="1" stop-color="${oklch(0.17, 0.025, H)}"/></linearGradient>
<linearGradient id="side" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${oklch(0.13, 0.02, H)}"/><stop offset="1" stop-color="${oklch(0.08, 0.015, H)}"/></linearGradient>
<linearGradient id="hi" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".14"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="tube" x1="146" y1="0" x2="334" y2="0" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="${a1}"/><stop offset="1" stop-color="${a2}"/></linearGradient>
<radialGradient id="node"><stop offset="0" stop-color="${hot}"/><stop offset=".55" stop-color="${a2}"/><stop offset="1" stop-color="${a1}"/></radialGradient>
<filter id="glow" filterUnits="userSpaceOnUse" x="0" y="0" width="512" height="512"><feGaussianBlur stdDeviation="7" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
<filter id="shadow" filterUnits="userSpaceOnUse" x="0" y="0" width="512" height="512"><feDropShadow dx="0" dy="16" stdDeviation="16" flood-color="#000" flood-opacity=".5"/></filter>
<clipPath id="tileClip"><rect width="512" height="512" rx="116"/></clipPath>
<clipPath id="faceClip"><rect x="117" y="86" width="250" height="330" rx="18"/></clipPath>
</defs>
<g clip-path="url(#tileClip)">
<rect width="512" height="512" fill="url(#tile)"/>
<g opacity=".9" fill="none" stroke="${a1}" stroke-opacity=".22" stroke-width="2">
<path d="M256 330 L-200 512 M256 330 L-30 512 M256 330 L110 512 M256 330 L256 512 M256 330 L402 512 M256 330 L542 512 M256 330 L712 512"/>
<path d="M0 352 H512 M0 380 H512 M0 420 H512 M0 476 H512"/>
</g>
<circle cx="242" cy="250" r="230" fill="url(#aura)"/>
</g>
<rect x="1" y="1" width="510" height="510" rx="115" fill="none" stroke="#fff" stroke-opacity=".1" stroke-width="2"/>
<rect x="129" y="100" width="250" height="330" rx="18" fill="url(#side)" filter="url(#shadow)"/>
<rect x="117" y="86" width="250" height="330" rx="18" fill="url(#face)"/>
<g clip-path="url(#faceClip)"><circle cx="242" cy="214" r="150" fill="url(#aura)"/><rect x="117" y="86" width="250" height="170" fill="url(#hi)"/></g>
<rect x="117" y="86" width="250" height="330" rx="18" fill="none" stroke="#fff" stroke-opacity=".22" stroke-width="2"/>
<circle cx="139" cy="106" r="5.5" fill="${oklch(0.11, 0.02, H)}" stroke="#fff" stroke-opacity=".16" stroke-width="1.5"/>
<circle cx="345" cy="106" r="5.5" fill="${oklch(0.11, 0.02, H)}" stroke="#fff" stroke-opacity=".16" stroke-width="1.5"/>
<path d="M117 326 H367" stroke="#000" stroke-opacity=".5" stroke-width="3"/>
<path d="M117 329.5 H367" stroke="#fff" stroke-opacity=".1" stroke-width="1.5"/>
<rect x="137" y="340" width="210" height="60" rx="10" fill="#000" fill-opacity=".22" stroke="#fff" stroke-opacity=".07" stroke-width="1.5"/>
<path d="M117 344 H127 M117 354 H127 M117 364 H127 M117 374 H127 M117 384 H127 M117 394 H127 M357 344 H367 M357 354 H367 M357 364 H367 M357 374 H367 M357 384 H367 M357 394 H367" stroke="#000" stroke-opacity=".45" stroke-width="3"/>
${tube('M146 214 L226 214')}
${tube('M226 214 C 270 214 290 160 334 160')}
${tube('M226 214 C 270 214 290 268 334 268')}
<circle cx="146" cy="214" r="6" fill="${a2}" filter="url(#glow)"/>
${end(160)}
${end(268)}
<circle cx="226" cy="214" r="18" fill="url(#node)" filter="url(#glow)"/>
<circle cx="226" cy="214" r="7" fill="${hot}"/>
</svg>`;
}

// The small design: the split line alone on a dark tile, drawn on a 64 grid.
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

const main = mainSvg();
writeFileSync(`${iconsDir}source.svg`, main);
writeFileSync(`${iconsDir}source-1024.png`, render(main, 1024));
console.log('Generated source.svg and source-1024.png');

execSync(`bun tauri icon "${iconsDir}source-1024.png"`, { stdio: 'inherit', cwd: guiDir });

// Small sizes use the small design, which stays readable where the main one turns to mud.
// Stroke widths per size come from the design (on its 64 grid).
const SW = { 16: 6, 24: 5.4, 32: 5, 48: 4.4, 64: 4 };
const small = (px) => render(smallSvg(px, SW[px]), px);
writeFileSync(`${iconsDir}32x32.png`, small(32));

// The Windows icon: the small design up to 64, the main one at 256. PNG entries, so the ICO
// is a header, a directory and the images back to back.
const entries = [16, 24, 32, 48, 64].map((px) => [px, small(px)]).concat([[256, render(main, 256)]]);
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

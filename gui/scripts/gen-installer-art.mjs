// Draws the Windows installer's sidebar (welcome and finish pages) and header images in the icon's
// colours, as the 24-bit BMPs NSIS needs.
import { readFileSync, writeFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { Resvg } from '@resvg/resvg-js';

const iconsDir = fileURLToPath(new URL('../src-tauri/icons/', import.meta.url));
const outDir = fileURLToPath(new URL('../src-tauri/windows/', import.meta.url));
const accent = '#bdadff';

// A worldline splitting at `x,y` into two that end `dx` further on, `dy` apart.
const split = (x, y, dx, dy) =>
  `M${x} ${y} C ${x + dx * 0.6} ${y} ${x + dx * 0.4} ${y - dy} ${x + dx} ${y - dy}` +
  ` M${x} ${y} C ${x + dx * 0.6} ${y} ${x + dx * 0.4} ${y + dy} ${x + dx} ${y + dy}`;

const sidebar = `<svg xmlns="http://www.w3.org/2000/svg" width="164" height="314" viewBox="0 0 164 314">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0.35" y2="1"><stop offset="0" stop-color="#292344"/><stop offset="1" stop-color="#070d1a"/></linearGradient>
    <radialGradient id="glow"><stop offset="0" stop-color="${accent}" stop-opacity="0.28"/><stop offset="1" stop-color="${accent}" stop-opacity="0"/></radialGradient>
  </defs>
  <rect width="164" height="314" fill="url(#bg)"/>
  <circle cx="58" cy="157" r="70" fill="url(#glow)"/>
  <g fill="none" stroke="${accent}" stroke-linecap="round">
    <path d="${split(118, 112, 60, 22)} ${split(118, 202, 60, 22)}" stroke-width="2" opacity="0.3"/>
    <path d="M-4 157 L58 157 ${split(58, 157, 60, 45)}" stroke-width="5"/>
  </g>
  <g fill="${accent}" opacity="0.3"><circle cx="178" cy="90" r="3"/><circle cx="178" cy="134" r="3"/><circle cx="178" cy="180" r="3"/><circle cx="178" cy="224" r="3"/></g>
  <g fill="${accent}"><circle cx="118" cy="112" r="5.5"/><circle cx="118" cy="202" r="5.5"/></g>
  <circle cx="58" cy="157" r="9" fill="${accent}" stroke="#1f1a35" stroke-width="4.5"/>
</svg>`;

// The header strip is white (NSIS draws it in the window colour), with the icon on the left and
// faint worldlines trailing off under the page title.
const icon = readFileSync(`${iconsDir}source.svg`, 'utf8')
  .replace(/^<svg[^>]*>/, '<svg x="8" y="8" width="41" height="41" viewBox="0 0 1024 1024">');
const header = `<svg xmlns="http://www.w3.org/2000/svg" width="150" height="57" viewBox="0 0 150 57">
  <defs><linearGradient id="fade" x1="0" x2="1"><stop offset="0" stop-color="#7c6bd6" stop-opacity="0.55"/><stop offset="1" stop-color="#7c6bd6" stop-opacity="0"/></linearGradient></defs>
  <rect width="150" height="57" fill="#ffffff"/>
  <path d="M56 28.5 L82 28.5 ${split(82, 28.5, 40, 11)}" fill="none" stroke="url(#fade)" stroke-width="2" stroke-linecap="round"/>
  ${icon}
</svg>`;

// Rows bottom-up, BGR, each padded to 4 bytes.
function bmp(width, height, rgba) {
  const stride = Math.ceil((width * 3) / 4) * 4;
  const out = Buffer.alloc(54 + stride * height);
  out.write('BM', 0);
  out.writeUInt32LE(out.length, 2);
  out.writeUInt32LE(54, 10);
  out.writeUInt32LE(40, 14);
  out.writeInt32LE(width, 18);
  out.writeInt32LE(height, 22);
  out.writeUInt16LE(1, 26);
  out.writeUInt16LE(24, 28);
  out.writeUInt32LE(stride * height, 34);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const src = (y * width + x) * 4;
      const dst = 54 + (height - 1 - y) * stride + x * 3;
      out[dst] = rgba[src + 2];
      out[dst + 1] = rgba[src + 1];
      out[dst + 2] = rgba[src];
    }
  }
  return out;
}

for (const [name, svg] of [['nsis-sidebar', sidebar], ['nsis-header', header]]) {
  const image = new Resvg(svg).render();
  writeFileSync(`${outDir}${name}.bmp`, bmp(image.width, image.height, image.pixels));
}

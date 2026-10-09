// The overlay window's page (`overlay.html`, ADR 0009): renders what Rust sends, nothing else.
import { createRoot } from 'react-dom/client';
import './overlay.css';
import { Overlay } from './Overlay';

async function start(): Promise<void> {
  // The text block is sized from the rendered text, so the fonts have to be in first.
  const fonts = ['500 12px Figtree', '600 14px Figtree', '600 14px "JetBrains Mono"'];
  await Promise.all(fonts.map((font) => document.fonts.load(font))).catch(() => undefined);

  const root = document.getElementById('root');
  if (!root) throw new Error('Failed to find the root element');
  createRoot(root).render(<Overlay />);
}

void start();

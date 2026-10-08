import { describe, expect, it } from 'vitest';
import { accentVars } from './accent';

describe('accentVars', () => {
  it('derives the accent palette from one hue', () => {
    expect(accentVars(150)).toEqual({
      '--acc': 'oklch(80% 0.13 150)',
      '--acc-ink': 'oklch(21% 0.06 150)',
      '--acc-soft': 'oklch(80% 0.13 150 / 0.12)',
      '--acc-line': 'oklch(80% 0.13 150 / 0.38)',
      '--acc-glow': 'oklch(52% 0.15 150 / 0.32)',
      '--acc-fill': 'oklch(56% 0.15 150)',
      '--acc-icon': 'linear-gradient(145deg, oklch(80% 0.13 150), oklch(56% 0.16 180))',
      '--acc-icon-ink': 'oklch(20% 0.05 150)',
    });
  });

  it('wraps the icon gradient hue', () => {
    expect(accentVars(350)['--acc-icon']).toContain('oklch(56% 0.16 20)');
  });
});

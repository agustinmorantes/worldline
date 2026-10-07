/** Brand hue: onboarding, and the chrome when no game is selected. */
export const BRAND_HUE = 290;

export type AccentVars = Record<`--acc${string}`, string>;

/** The per-game accent palette from one hue (`pal()` in the mocks), as CSS variables. */
export function accentVars(hue: number): AccentVars {
  const h = hue.toString();
  const h2 = ((hue + 30) % 360).toString();
  return {
    '--acc': `oklch(80% 0.13 ${h})`,
    '--acc-ink': `oklch(21% 0.06 ${h})`,
    '--acc-soft': `oklch(80% 0.13 ${h} / 0.12)`,
    '--acc-line': `oklch(80% 0.13 ${h} / 0.38)`,
    '--acc-glow': `oklch(52% 0.15 ${h} / 0.32)`,
    '--acc-fill': `oklch(56% 0.15 ${h})`,
    '--acc-icon': `linear-gradient(145deg, oklch(80% 0.13 ${h}), oklch(56% 0.16 ${h2}))`,
  };
}

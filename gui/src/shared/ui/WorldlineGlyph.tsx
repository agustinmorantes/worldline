import type { ReactElement } from 'react';

/** The small app mark: a line that splits in two, in the current accent. */
export function WorldlineGlyph({ size = 20 }: { size?: number }): ReactElement {
  return (
    <span
      aria-hidden="true"
      className="block shrink-0 overflow-hidden rounded-[4.5px] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.08)]"
      style={{
        width: size,
        height: size,
        background: 'linear-gradient(165deg, color-mix(in oklch, var(--acc) 30%, oklch(20% 0.03 265)), oklch(15% 0.02 265))',
      }}
    >
      <svg width={size} height={size} viewBox="0 0 64 64" fill="none" strokeLinecap="round" strokeLinejoin="round" className="block">
        <path
          d="M10 32 L26 32 M26 32 C 36 32 40 18 52 18 M26 32 C 36 32 40 46 52 46"
          strokeWidth={5.8}
          className="transition-[stroke] duration-600"
          style={{ stroke: 'var(--acc)' }}
        />
        <circle cx="52" cy="18" r="3.4" style={{ fill: 'var(--acc)' }} />
        <circle cx="52" cy="46" r="3.4" style={{ fill: 'var(--acc)' }} />
        <circle
          cx="26"
          cy="32"
          r="6"
          strokeWidth={4.35}
          style={{ fill: 'var(--acc)', stroke: 'color-mix(in oklch, var(--acc) 22%, oklch(18% 0.02 265))' }}
        />
      </svg>
    </span>
  );
}

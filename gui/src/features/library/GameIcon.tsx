import type { CSSProperties, ReactElement } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { accentVars } from '@/shared/tokens/accent';
import { cn } from '@/shared/ui/utils';
import { initials, titleHue } from './library';

interface GameIconProps {
  title: string;
  /** The game's accent hue; scan results without one get a hue from the title. */
  hue?: number;
  size?: number;
  iconPath?: string;
  className?: string;
  style?: CSSProperties;
  /** Marks the header icon that sidebar icons fly into. */
  'data-icon-dock'?: number;
}

/** The game's icon, or its initials on the accent gradient. */
export function GameIcon({ title, hue, size = 36, iconPath, className, style, ...data }: GameIconProps): ReactElement {
  const box: CSSProperties = {
    width: size,
    height: size,
    borderRadius: Math.round(size * 0.305),
    ...style,
  };

  if (iconPath !== undefined) {
    // A dark tile in the game's hue, so icons with transparent corners still read as a tile.
    const h = hue ?? titleHue(title);
    return (
      <span
        {...data}
        className={cn('block shrink-0 overflow-hidden', className)}
        style={{
          background: `linear-gradient(145deg, oklch(34% 0.06 ${h.toString()}), oklch(24% 0.05 ${((h + 30) % 360).toString()}))`,
          ...box,
        }}
      >
        <img src={convertFileSrc(iconPath)} alt="" className="size-full object-cover" draggable={false} />
      </span>
    );
  }

  const palette = accentVars(hue ?? titleHue(title));
  return (
    <span
      {...data}
      aria-hidden="true"
      className={cn('flex shrink-0 items-center justify-center font-display font-extrabold', className)}
      style={{
        background: palette['--acc-icon'],
        color: palette['--acc-icon-ink'],
        fontSize: Math.round(size * 0.38),
        ...box,
      }}
    >
      {initials(title)}
    </span>
  );
}

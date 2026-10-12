import { useEffect, useRef, type CSSProperties, type ReactElement } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { motion } from 'motion/react';
import { spring, useMotionReduced } from '@/shared/motion/motion';
import { accentVars } from '@/shared/tokens/accent';
import { cn } from '@/shared/ui/utils';
import { initials, titleHue } from './library';

interface GameIconProps {
  title: string;
  /** The game's accent hue; scan results without one get a hue from the title. */
  hue?: number;
  size?: number;
  iconPath?: string;
  /** The icon's content hash: a new one reloads an icon replaced at the same path. */
  iconRevision?: string;
  className?: string;
  style?: CSSProperties;
  /** Marks the header icon that sidebar icons fly into. */
  'data-icon-dock'?: number;
}

/** The game's icon, or its initials on the accent gradient. An icon that arrives while shown
 * (filled from an install in the background) settles into the tile instead of popping in. */
export function GameIcon({ title, hue, size = 36, iconPath, iconRevision, className, style, ...data }: GameIconProps): ReactElement {
  const reduced = useMotionReduced();
  const hadIconRef = useRef(iconPath !== undefined);
  const arrived = iconPath !== undefined && !hadIconRef.current && !reduced;
  useEffect(() => {
    if (iconPath !== undefined) hadIconRef.current = true;
  }, [iconPath]);
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
        <motion.img
          src={iconSrc(iconPath, iconRevision)}
          alt=""
          className="size-full object-cover"
          draggable={false}
          initial={arrived ? { opacity: 0, scale: 0.6 } : false}
          animate={{ opacity: 1, scale: 1 }}
          transition={spring}
        />
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

/** The icon file's URL. The asset protocol ignores the query, so the revision only tells the
 * webview a replaced icon apart from the one it cached at the same path. */
function iconSrc(path: string, revision: string | undefined): string {
  const src = convertFileSrc(path);
  return revision === undefined ? src : `${src}?v=${encodeURIComponent(revision)}`;
}

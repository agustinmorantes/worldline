import { ReactElement } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';

interface GameIconProps {
  title: string;
  size?: number;
  iconPath?: string;
}

export function GameIcon({ title, size = 32, iconPath }: GameIconProps): ReactElement {
  const style = { width: size, height: size, minWidth: size, minHeight: size };

  if (iconPath !== undefined) {
    return (
      <div
        className="rounded-md shrink-0 shadow-sm overflow-hidden"
        style={style}
      >
        <img
          src={convertFileSrc(iconPath)}
          alt={title}
          className="w-full h-full object-cover"
        />
      </div>
    );
  }

  const hue = (title.split('').reduce((a, c) => a + c.charCodeAt(0), 0) * 137.5) % 360;

  return (
    <div
      className="flex items-center justify-center rounded-md font-bold text-white shrink-0 shadow-sm"
      style={{
        ...style,
        fontSize: size * 0.5,
        backgroundColor: `oklch(64% 0.22 ${hue.toString()}deg)`,
      }}
    >
      {title.charAt(0).toUpperCase()}
    </div>
  );
}

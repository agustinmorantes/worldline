import type { ReactElement } from 'react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { GameIcon } from './GameIcon';
import { useShellNav } from './shell-nav';

/** The game header's icon: the one the sidebar row's icon flies into. */
export function DockedGameIcon({ game, size = 72 }: { game: FullGame; size?: number }): ReactElement {
  const { pending, flying } = useShellNav();
  // Hidden while its icon is in flight, and while arriving before that flight takes off.
  const away = flying.out === game.id || flying.home === game.id || (pending?.view === 'game' && pending.gameId === game.id);
  return (
    <GameIcon
      title={game.title}
      hue={game.accent_hue}
      iconPath={game.icon_path ?? undefined}
      size={size}
      className="shadow-[0_18px_40px_-12px_oklch(0%_0_0/0.6)]"
      style={{ opacity: away ? 0 : 1 }}
      data-icon-dock={game.id}
    />
  );
}

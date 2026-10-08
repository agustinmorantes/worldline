// The game view's header: icon, title, the playthrough/sync/options chips, the Sessions/Graph
// toggle and the approximate play time.
import type { ReactElement } from 'react';
import { motion } from 'motion/react';
import { Cloud, CloudCheck } from 'lucide-react';
import type { DeviceKind } from '@/ipc/bindings/DeviceKind';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { DockedGameIcon } from '@/features/library/DockedGameIcon';
import { useT } from '@/shared/i18n';
import { spring } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { cn } from '@/shared/ui/utils';
import type { SyncChip } from './game-view';
import { GameMenu } from './GameMenu';
import { chip, PlaythroughPicker } from './PlaythroughPicker';

export type GameView = 'sessions' | 'graph';

export function GameHeader({ game, sync, syncDevice, onSyncChip, played, sessions, view, onView, busy, onSwitchSlot, onCreateSlot }: {
  game: FullGame;
  sync: SyncChip;
  /** What kind of device the chip names. */
  syncDevice: DeviceKind;
  onSyncChip: () => void;
  /** Formatted play time, or `null` before the first checkpoint. */
  played: string | null;
  sessions: number;
  view: GameView;
  onView: (view: GameView) => void;
  busy: boolean;
  onSwitchSlot: (slotId: number) => void;
  onCreateSlot: () => void;
}): ReactElement {
  const t = useT();

  return (
    <header className="flex items-start gap-5 px-8 pb-6 pt-6">
      <DockedGameIcon game={game} />
      <div className="min-w-0 flex-1">
        <h1 className="truncate font-display text-[34px] font-extrabold leading-[1.15]">{game.title}</h1>
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <PlaythroughPicker game={game} disabled={busy} onSwitch={onSwitchSlot} onCreate={onCreateSlot} />
          {sync.kind !== 'off' && <SyncChipButton sync={sync} device={syncDevice} onClick={onSyncChip} />}
          <GameMenu game={game} />
          <ViewToggle view={view} onView={onView} />
        </div>
      </div>
      <div className="shrink-0 text-right">
        <div className="font-mono text-[26px] font-semibold">{played === null ? '—' : t.game.played(played)}</div>
        <div className="mt-1 text-[12.5px] text-tx3">{played === null ? t.game.nothingYet : t.game.playedSub(sessions)}</div>
      </div>
    </header>
  );
}

function SyncChipButton({ sync, device, onClick }: { sync: Exclude<SyncChip, { kind: 'off' }>; device: DeviceKind; onClick: () => void }): ReactElement {
  const t = useT();
  const deck = sync.kind === 'new-from' || sync.kind === 'played-too';
  const text = sync.kind === 'new-from'
    ? t.gameStatus.newFrom(sync.device)
    : sync.kind === 'played-too'
      ? t.gameStatus.playedToo(sync.device)
      : sync.kind === 'sending'
        ? t.game.uploading
        : sync.kind === 'not-in-cloud'
          ? t.gameStatus.notInCloud
          : t.game.inCloud;

  return (
    <button type="button" onClick={onClick} className={cn(chip, deck && 'text-deck', sync.kind === 'sending' && 'text-tx')}>
      {deck && <DeviceGlyph kind={device} size={15} />}
      {sync.kind === 'in-cloud' && <CloudCheck size={16} aria-hidden="true" />}
      {sync.kind === 'not-in-cloud' && <Cloud size={16} strokeDasharray="3 3" aria-hidden="true" />}
      {sync.kind === 'sending' && (
        <span aria-hidden="true" className="relative h-1.5 w-[22px]">
          <span className="absolute top-0 size-1.5 rounded-full bg-tx [animation:travel_600ms_ease-in-out_infinite_alternate]" />
        </span>
      )}
      {text}
    </button>
  );
}

function ViewToggle({ view, onView }: { view: GameView; onView: (view: GameView) => void }): ReactElement {
  const t = useT();
  const button = 'relative z-[1] flex h-6 w-8 items-center justify-center rounded-full text-tx3 transition-[color,transform] duration-200 ease-spring hover:text-tx active:scale-90 aria-pressed:text-acc-ink disabled:pointer-events-none disabled:opacity-40';

  return (
    <div role="group" aria-label={t.game.timelineView} className="relative ml-auto flex h-[30px] gap-0.5 rounded-full bg-el p-[3px]">
      <motion.span
        aria-hidden="true"
        className="absolute top-[3px] h-6 w-8 rounded-full bg-acc"
        animate={{ left: view === 'sessions' ? 3 : 37 }}
        transition={spring}
      />
      <button type="button" aria-pressed={view === 'sessions'} aria-label={t.game.sessionsView} title={t.game.sessionsView} className={button} onClick={() => { onView('sessions'); }}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden="true"><path d="M4 6h9M4 12h16M4 18h6" /></svg>
      </button>
      {/* The graph arrives in P3. */}
      <button type="button" disabled aria-pressed={view === 'graph'} aria-label={t.game.graphView} title={t.game.graphView} className={button} onClick={() => { onView('graph'); }}>
        <svg width="17" height="17" viewBox="0 0 64 64" fill="none" stroke="currentColor" strokeWidth="6" strokeLinecap="round" aria-hidden="true" style={{ rotate: '-90deg' }}>
          <path d="M10 32 L26 32 M26 32 C 36 32 40 18 52 18 M26 32 C 36 32 40 46 52 46" />
          <circle cx="52" cy="18" r="3.4" fill="currentColor" stroke="none" />
          <circle cx="52" cy="46" r="3.4" fill="currentColor" stroke="none" />
          <circle cx="26" cy="32" r="6" fill="currentColor" stroke="none" />
        </svg>
      </button>
    </div>
  );
}

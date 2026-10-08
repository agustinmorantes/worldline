// The selected checkpoint: when and where it was made, its name, and the hold-to-rewind button.
import { type ReactElement, useState } from 'react';
import { motion } from 'motion/react';
import { ArrowDown, X } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import { GameIcon } from '@/features/library/GameIcon';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatRelative, formatTime, useLocale, useT } from '@/shared/i18n';
import { ease } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { HoldButton } from '@/shared/ui/HoldButton';
import { formatDuration } from './Timeline';
import { timeOf } from './sessions';

export function CheckpointPanel({ game, version, active, onLine, device, lessPlayMs, aheadDevice, busy, onName, onRewind, onClose, now }: {
  game: FullGame;
  version: Version;
  /** The checkpoint you are on. */
  active: Version | null;
  /** On your current line: rewinding, not switching timelines. */
  onLine: boolean;
  device: CheckpointDevice;
  /** Play since this checkpoint, on your line. */
  lessPlayMs: number;
  /** Another device's progress that would move aside. */
  aheadDevice: string | null;
  busy: boolean;
  onName: (tag: string) => void;
  onRewind: () => void;
  /** Present when the panel is a drawer. */
  onClose?: () => void;
  now: Date;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const here = active?.id === version.id;
  const [draft, setDraft] = useState(version.tag ?? '');
  // Another checkpoint, or the name changed underneath: start the field over.
  const source = `${version.id.toString()}:${version.tag ?? ''}`;
  const [shown, setShown] = useState(source);
  if (shown !== source) {
    setShown(source);
    setDraft(version.tag ?? '');
  }

  const date = new Date(timeOf(version));
  const tagged = version.tag !== null && version.tag !== '';
  const title = tagged ? (version.tag ?? '') : t.sessions.autosave;
  const eyebrow = here ? t.checkpoint.youAreHere : onLine ? t.checkpoint.rewindTo : t.checkpoint.switchTo;
  const tint = here ? 'var(--acc)' : onLine ? 'var(--tx3)' : 'var(--deck)';

  function commit(): void {
    const next = draft.trim();
    if (next !== (version.tag ?? '')) onName(next);
  }

  return (
    <motion.div key={version.id} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.38, ease }}>
      {!here && active !== null && (
        <>
          <div className="flex h-11 items-center gap-2.5 rounded-[14px] bg-sf px-3.5">
            <GameIcon title={game.title} hue={game.accent_hue} iconPath={game.icon_path ?? undefined} size={22} className="text-[9px]" />
            <span className="text-[13px] font-bold">{t.checkpoint.now}</span>
            <span className="min-w-0 flex-1 truncate text-[13px] text-tx2">{active.tag ?? t.sessions.autosave}</span>
            <span className="font-mono text-xs text-tx3">{formatRelative(new Date(timeOf(active)), locale, now)}</span>
          </div>
          <div className="relative flex h-[46px] items-center">
            <span aria-hidden="true" className="absolute inset-y-1 left-6 border-l-2 border-dashed border-acc-line" />
            {onLine && lessPlayMs >= 60_000 && (
              <span className="ml-[42px] flex h-[26px] items-center gap-[7px] rounded-lg bg-acc-soft px-2.5 font-mono text-[12.5px] font-semibold text-acc">
                <ArrowDown size={13} strokeWidth={2.6} aria-hidden="true" />
                {t.checkpoint.lessPlay(formatDuration(lessPlayMs, t))}
              </span>
            )}
          </div>
        </>
      )}
      <div className="overflow-hidden rounded-[20px] bg-sf shadow-[0_24px_48px_-24px_oklch(0%_0_0/0.6)]">
        <div className="h-1 transition-[background] duration-500" style={{ background: tint }} />
        <div className="px-[18px] pb-[18px] pt-4">
          <div className="mb-1.5 flex items-baseline gap-2.5">
            <span className="text-[11px] font-bold uppercase tracking-[0.12em]" style={{ color: tint }}>{eyebrow}</span>
            <span className="ml-auto font-mono text-xs text-tx3">{t.checkpoint.number(version.version_number)}</span>
            {onClose !== undefined && (
              <button type="button" aria-label={t.checkpoint.close} onClick={onClose} className="-mr-1 self-center rounded-md p-1 text-tx3 hover:text-tx min-[1000px]:hidden">
                <X size={15} aria-hidden="true" />
              </button>
            )}
          </div>
          <div className="truncate font-display text-2xl font-extrabold">{title}</div>
          <div className="mt-1.5 flex items-center gap-[7px] text-[13px] text-tx2">
            <span className="font-mono">
              {formatTime(date, locale)} · {formatRelative(date, locale, now)}
            </span>
            <span aria-hidden="true" className="size-[3px] rounded-full bg-tx3" />
            <span className={device.here ? 'flex' : 'flex text-deck'}><DeviceGlyph kind={device.kind} size={14} /></span>
            <span className="truncate">{device.here ? t.checkpoint.thisDevice : (device.name ?? t.sessions.anotherDevice)}</span>
          </div>
          <label className="mt-3.5 flex h-10 items-center gap-2.5 rounded-xl bg-el px-3 text-sm text-acc transition-shadow duration-200 focus-within:shadow-[inset_0_0_0_1.5px_var(--acc-line)]">
            <span aria-hidden="true" className="size-2 shrink-0 rotate-45 bg-current" />
            <input
              value={draft}
              onChange={(e) => { setDraft(e.target.value); }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur();
                if (e.key === 'Escape') { setDraft(version.tag ?? ''); e.stopPropagation(); }
              }}
              onBlur={commit}
              placeholder={t.checkpoint.namePlaceholder}
              aria-label={t.checkpoint.nameLabel}
              className="min-w-0 flex-1 bg-transparent font-semibold text-tx outline-none placeholder:font-normal placeholder:text-tx3"
            />
          </label>
        </div>
      </div>
      {!here && (
        <>
          <HoldButton
            label={onLine ? t.checkpoint.holdToRewind : t.checkpoint.holdToSwitch}
            hint={t.checkpoint.holdHint}
            disabled={busy}
            onComplete={onRewind}
            rumble
          />
          {aheadDevice !== null && (
            <div className="mt-3 flex items-center gap-2 text-[13px] font-semibold text-warn">
              <span aria-hidden="true" className="size-[7px] shrink-0 rounded-full bg-warn" />
              {t.checkpoint.aheadWarning(aheadDevice)}
            </div>
          )}
        </>
      )}
    </motion.div>
  );
}

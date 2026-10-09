// What the Sessions and Graph views share: the scrolling frame with arrow-key navigation, the
// checkpoint dots and the "You are here" token (one layout id each, so they fly between the
// views), another device's ghost row, and the empty state.
import { type KeyboardEvent, type ReactElement, type ReactNode, useState } from 'react';
import { motion } from 'motion/react';
import { convertFileSrc } from '@tauri-apps/api/core';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import { GameIcon } from '@/features/library/GameIcon';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatRelative, formatTime, type Messages, useLocale, useT } from '@/shared/i18n';
import { ease, spring } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { useScreenshotPreview } from './queries';
import { dayLabel, durationParts, timeOf } from './sessions';

export function formatDuration(ms: number, t: Messages): string {
  const { hours, minutes } = durationParts(ms);
  return t.sessions.duration(hours, minutes);
}

const dotId = (id: number): string => `checkpoint-${id.toString()}`;

export function TimelineFrame({ order, selectedId, activeId, onSelect, rewinding, children }: {
  /** Checkpoints in keyboard order. */
  order: number[];
  selectedId: number | null;
  activeId: number | null;
  onSelect: (id: number) => void;
  /** A rewind is running: the streaks play over the list. */
  rewinding: boolean;
  children: ReactNode;
}): ReactElement {
  const t = useT();

  function onKeyDown(event: KeyboardEvent): void {
    if (order.length === 0) return;
    const current = order.indexOf(selectedId ?? activeId ?? order[0]);
    let next: number;
    if (event.key === 'ArrowDown' || event.key === 'ArrowLeft') next = Math.min(order.length - 1, current + 1);
    else if (event.key === 'ArrowUp' || event.key === 'ArrowRight') next = Math.max(0, current - 1);
    else if (event.key === 'Home' || event.key === 'Escape') next = activeId === null ? 0 : order.indexOf(activeId);
    else return;
    event.preventDefault();
    if (next < 0) return;
    onSelect(order[next]);
    document.getElementById(dotId(order[next]))?.scrollIntoView({ block: 'nearest' });
  }

  return (
    <div className="relative min-h-0 flex-1">
      <motion.div
        layoutScroll
        tabIndex={0}
        onKeyDown={onKeyDown}
        aria-label={t.sessions.listLabel}
        className="size-full overflow-y-auto overflow-x-hidden rounded-2xl pb-16 outline-none [scrollbar-color:var(--ln)_transparent] [scrollbar-width:thin] focus-visible:shadow-[inset_0_0_0_2px_var(--acc-line)]"
      >
        {children}
      </motion.div>
      {rewinding && <RewindStreaks />}
    </div>
  );
}

export function Diamond({ size = 6 }: { size?: number }): ReactElement {
  return <span aria-hidden="true" className="shrink-0 rotate-45 bg-current" style={{ width: size, height: size }} />;
}

export function Dot({ game, version, left, top, side, elsewhere, graph = false, active, selected, landed, onSelect, now }: {
  game: FullGame;
  version: Version;
  /** Centre of the dot within its positioned parent. */
  left: string | number;
  /** Centre of the dot; vertically centred when absent. */
  top?: number;
  /** On another timeline. */
  side: boolean;
  /** Made on another device. */
  elsewhere: boolean;
  /** In the Graph view, other timelines keep a hint of the other device's color. */
  graph?: boolean;
  active: boolean;
  selected: boolean;
  landed: boolean;
  onSelect: (id: number) => void;
  now: Date;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const tagged = version.tag !== null && version.tag !== '';
  const date = new Date(timeOf(version));
  const name = version.retired ? t.sessions.cleanedUp : tagged ? (version.tag ?? '') : t.sessions.autosave;
  const size = version.retired ? 7 : tagged ? 11 : selected && !active ? 12 : 9;
  const fill = version.retired
    ? 'transparent'
    : side
      ? (graph && elsewhere ? 'oklch(80% 0.11 205 / 0.75)' : 'var(--tx3)')
      : elsewhere ? 'var(--deck)' : 'var(--acc)';
  const place = { left, top: top ?? '50%' };
  const label = `${formatTime(date, locale)} · ${name}`;

  return (
    <>
      <Tooltip mono label={label} content={version.retired ? label : <DotPreview versionId={version.id} label={label} />}>
        <motion.button
          id={dotId(version.id)}
          layoutId={`dot-${version.id.toString()}`}
          type="button"
          tabIndex={-1}
          disabled={version.retired}
          aria-pressed={selected}
          aria-label={t.sessions.dotLabel(name, `${dayLabel(date, now, locale, t.sessions)} ${formatTime(date, locale)}`)}
          onClick={() => { onSelect(version.id); }}
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
          transition={{ scale: spring, layout: { duration: 0.48, ease } }}
          className="group absolute z-[2] -mt-[11px] -ml-[11px] flex size-[22px] items-center justify-center rounded-full"
          style={place}
        >
          <span
            className="block shrink-0 border-2 transition-[transform,width,height,border-radius,background-color,border-color] duration-[260ms] ease-spring group-enabled:group-hover:scale-[1.7]"
            style={{
              width: size,
              height: size,
              borderRadius: tagged ? 2 : '50%',
              rotate: tagged ? '45deg' : '0deg',
              backgroundColor: fill,
              borderColor: version.retired ? 'var(--tx3)' : selected && !active ? 'var(--tx)' : 'transparent',
            }}
          />
        </motion.button>
      </Tooltip>
      {active && (
        <motion.span
          layoutId={`here-${game.id.toString()}`}
          aria-hidden="true"
          className="pointer-events-none absolute z-[3] -mt-[15px] -ml-[15px] size-[30px]"
          style={place}
          transition={{ duration: 0.64, ease }}
        >
          <span className="absolute inset-0 rounded-xl border-2 border-acc [animation:halo_2.4s_ease-out_infinite]" />
          <span className="absolute inset-0 rounded-xl border-2 border-acc [animation:halo_2.4s_ease-out_1.2s_infinite]" />
          <GameIcon
            title={game.title}
            hue={game.accent_hue}
            iconPath={game.icon_path ?? undefined}
            size={30}
            className="relative shadow-[0_6px_18px_oklch(0%_0_0/0.5)] [&]:rounded-[10px] text-[11px]"
          />
          {landed && <span className="absolute inset-0 rounded-full border-2 border-acc [animation:burst_760ms_var(--ez)_both]" />}
        </motion.span>
      )}
    </>
  );
}

/**
 * A dot's tooltip: its time and name, under the checkpoint's screenshot once that has loaded.
 * Mounted only while the tooltip is open, so a long history asks for one screenshot per hover.
 */
function DotPreview({ versionId, label }: { versionId: number; label: string }): ReactElement {
  const { data: path } = useScreenshotPreview(versionId);
  const src = typeof path === 'string' ? convertFileSrc(path) : null;
  /** The source that finished loading; until then, and for a broken file, only the text shows. */
  const [loaded, setLoaded] = useState<string | null>(null);
  return (
    <>
      {src !== null && (
        <img
          src={src}
          alt=""
          width={240}
          height={135}
          decoding="async"
          draggable={false}
          onLoad={() => { setLoaded(src); }}
          className={cn('mb-1.5 mt-1 aspect-video w-[240px] rounded-md object-cover', loaded === src ? 'block animate-in fade-in-0 duration-200' : 'hidden')}
        />
      )}
      {label}
    </>
  );
}

export function GhostRow({ versions, device, onContinue, busy, running, now }: {
  versions: Version[];
  device: CheckpointDevice;
  onContinue: () => void;
  busy: boolean;
  /** The game is running: bringing progress here would write under it. */
  running: boolean;
  now: Date;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const first = timeOf(versions[0]);
  const last = timeOf(versions[versions.length - 1]);
  const name = device.name ?? t.sessions.anotherDevice;

  return (
    <motion.div
      layout="position"
      initial={{ opacity: 0, y: -10 }}
      animate={busy ? { opacity: 1, y: 12, scale: 0.97 } : { opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 12, scale: 0.97 }}
      transition={{ duration: 0.56, ease }}
      className={cn(
        'relative mb-4 ml-3.5 mr-3 mt-2.5 flex h-[74px] items-center gap-4 rounded-2xl border-[1.5px] px-4 transition-colors duration-400',
        busy ? 'border-solid border-deck bg-[oklch(80%_0.11_205/0.18)]' : 'border-dashed border-deck bg-[oklch(80%_0.11_205/0.06)]',
      )}
    >
      <span className="flex min-w-0 flex-col gap-2">
        <span className="flex items-center gap-2 text-sm font-bold text-deck">
          <DeviceGlyph kind={device.kind} size={16} />
          <span className="truncate">{t.sessions.playedOn(name)}</span>
        </span>
        <span className="font-mono text-xs text-tx2">
          {formatDuration(last - first, t)} · {formatRelative(new Date(last), locale, now)}
        </span>
      </span>
      <span
        aria-hidden="true"
        className="h-2 min-w-10 shrink rounded-full shadow-[inset_0_0_0_1.5px_oklch(80%_0.11_205/0.45)]"
        style={{
          flexBasis: Math.round(Math.max(40, Math.min(180, 40 + ((last - first) / (240 * 60_000)) * 140))),
          background: 'radial-gradient(circle, var(--deck) 2.5px, transparent 3.5px) 4px 50% / 22px 8px repeat-x',
        }}
      />
      <button
        type="button"
        disabled={busy || running}
        onClick={onContinue}
        className="ml-auto h-[38px] shrink-0 rounded-xl bg-deck px-[18px] text-sm font-bold text-[oklch(20%_0.05_205)] transition-[transform,filter] duration-[180ms] ease-spring hover:-translate-y-px hover:brightness-110 active:scale-95 disabled:opacity-60"
      >
        {t.sessions.continueHere}
      </button>
    </motion.div>
  );
}

export function EmptyGame({ title }: { title: string }): ReactElement {
  const t = useT();
  return (
    <motion.div
      initial={{ opacity: 0, y: 10, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.38, ease }}
      className="flex flex-1 flex-col items-center pt-[120px] text-center"
    >
      <div className="relative mb-7 h-2 w-[260px] rounded-full shadow-[inset_0_0_0_1.5px_var(--ln)]">
        <span className="absolute -top-px size-2.5 rounded-full bg-acc [animation:drift_2.4s_var(--ez)_infinite_alternate]" />
      </div>
      <h2 className="font-display text-2xl font-extrabold">{t.sessions.waitingFor(title)}</h2>
      <p className="mt-2 text-sm text-tx2">{t.sessions.emptyHint}</p>
    </motion.div>
  );
}

const STREAKS = [40, 120, 210, 262, 330, 420, 486, 560].map((x, i) => ({ x, y: (i * 97) % 420, h: 80 + ((i * 37) % 90), d: (i * 53) % 220 }));

/** The rewind's VHS-style streaks over the list. */
function RewindStreaks(): ReactElement {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 z-[4] overflow-hidden">
      {STREAKS.map((s) => (
        <span
          key={s.x}
          className="absolute w-0.5 rounded-sm bg-[linear-gradient(transparent,var(--acc),transparent)]"
          style={{ left: s.x, top: s.y, height: s.h, animation: `streak 640ms var(--ez) ${s.d.toString()}ms both` }}
        />
      ))}
    </div>
  );
}

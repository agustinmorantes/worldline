// The Sessions view's main column: your current line grouped into play sessions under day
// headers, other timelines folded into chips at their fork points, another device's pending
// progress as a ghost row, and the "You are here" token.
import { type KeyboardEvent, type ReactElement, useMemo } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { ChevronDown, CornerDownRight } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import { GameIcon } from '@/features/library/GameIcon';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatRelative, formatTime, type Messages, useLocale, useT } from '@/shared/i18n';
import { ease, spring } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { cn } from '@/shared/ui/utils';
import {
  dayLabel,
  durationParts,
  type Fork,
  navOrder,
  playedMs,
  type SessionsLayout,
  timeOf,
  trackPosition,
  trackWidth,
} from './sessions';

export function formatDuration(ms: number, t: Messages): string {
  const { hours, minutes } = durationParts(ms);
  return t.sessions.duration(hours, minutes);
}

/** Time column + device column, before a track starts. */
const TRACK_LEFT = 96;
/** Extra indent of the rows inside an open fork. */
const DRAWER_INDENT = 34;

interface SessionListProps {
  game: FullGame;
  layout: SessionsLayout;
  /** Every version of the playthrough, for play time. */
  versions: Version[];
  gapMs: number;
  activeId: number | null;
  selectedId: number | null;
  onSelect: (id: number) => void;
  openForks: ReadonlySet<number>;
  onToggleFork: (rootId: number) => void;
  deviceOf: (version: Version) => CheckpointDevice;
  /** Another device's progress waiting here: the ghost row's button. */
  ghost: { onContinue: () => void; busy: boolean } | null;
  /** A rewind is running: the streaks play over the list. */
  rewinding: boolean;
  /** Where the last rewind landed, for the burst. */
  landedId: number | null;
  now: Date;
}

export function SessionList(props: SessionListProps): ReactElement {
  const { game, layout, activeId, selectedId, onSelect, openForks, ghost, deviceOf, rewinding, now } = props;
  const t = useT();
  const locale = useLocale();
  const order = useMemo(() => navOrder(layout, openForks), [layout, openForks]);

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

  if (layout.days.length === 0) return <EmptyGame title={game.title} />;

  return (
    <div className="relative min-h-0 flex-1">
      <motion.div
        layoutScroll
        tabIndex={0}
        onKeyDown={onKeyDown}
        aria-label={t.sessions.listLabel}
        className="size-full overflow-y-auto overflow-x-hidden rounded-2xl pb-16 outline-none [scrollbar-color:var(--ln)_transparent] [scrollbar-width:thin] focus-visible:shadow-[inset_0_0_0_2px_var(--acc-line)]"
      >
        <AnimatePresence initial={false}>
          {layout.ghost !== null && ghost !== null && (
            <GhostRow key="ghost" versions={layout.ghost} device={deviceOf(layout.ghost[0])} {...ghost} now={now} />
          )}
          {layout.days.map((day) => (
            <section key={day.day} aria-label={dayLabel(new Date(day.day), now, locale, t.sessions)}>
              <h3 className="flex h-8 items-end pb-2 pl-3.5 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3">
                {dayLabel(new Date(day.day), now, locale, t.sessions)}
              </h3>
              {day.rows.map((row) => (
                <div key={row.versions[0].id} className="pb-1">
                  <SessionRowView {...props} session={row.versions} durationMs={row.durationMs} side={false} />
                  {row.forks.map((fork) => (
                    <ForkView key={fork.root.id} {...props} fork={fork} session={row.versions} open={openForks.has(fork.root.id)} />
                  ))}
                </div>
              ))}
            </section>
          ))}
        </AnimatePresence>
      </motion.div>
      {rewinding && <RewindStreaks />}
    </div>
  );
}

const dotId = (id: number): string => `checkpoint-${id.toString()}`;

function SessionRowView({ game, session, durationMs, side, activeId, selectedId, onSelect, deviceOf, landedId, now }: SessionListProps & {
  session: Version[];
  durationMs: number;
  /** A row of another timeline, inside its open fork. */
  side: boolean;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const device = deviceOf(session[0]);
  const elsewhere = !device.here && session.every((v) => !deviceOf(v).here);
  const tagged = session.filter((v) => v.tag !== null && v.tag !== '');
  const width = trackWidth(durationMs);
  const holdsSelection = selectedId !== null && session.some((v) => v.id === selectedId);

  return (
    <motion.div
      layout="position"
      initial={{ opacity: 0, y: -10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.97 }}
      transition={{ duration: 0.48, ease }}
      className={cn('relative flex h-14 items-center pr-3', side ? 'pl-[48px]' : 'pl-3.5')}
    >
      {holdsSelection && (
        <motion.span
          layoutId={`selection-${game.id.toString()}`}
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-0.5 right-0 rounded-[14px] bg-acc-soft shadow-[inset_0_0_0_1px_var(--acc-line)]"
          style={{ left: side ? 38 : 4 }}
          transition={{ duration: 0.38, ease }}
        />
      )}
      <span className="relative flex w-[72px] shrink-0 flex-col gap-[3px] font-mono">
        <span className={cn('text-[13px] font-semibold', side ? 'text-tx2' : 'text-tx')}>{formatTime(new Date(timeOf(session[0])), locale)}</span>
        <span className="text-[11.5px] text-tx3">{formatDuration(durationMs, t)}</span>
      </span>
      <span className="relative flex w-6 shrink-0 text-deck" title={elsewhere ? t.sessions.playedOn(device.name ?? t.sessions.anotherDevice) : undefined}>
        {elsewhere && <DeviceGlyph kind={device.kind} size={17} />}
      </span>
      <span
        className={cn('relative h-2 min-w-10 shrink rounded-full transition-[background-color] duration-400', side && 'shadow-[inset_0_0_0_1.5px_var(--ln)]')}
        style={{
          flexBasis: width,
          backgroundColor: side ? 'transparent' : elsewhere ? 'oklch(80% 0.11 205 / 0.2)' : 'color-mix(in oklch, var(--acc) 20%, transparent)',
        }}
      >
        {session.map((version) => (
          <Dot
            key={version.id}
            game={game}
            version={version}
            position={trackPosition(version, session)}
            side={side}
            elsewhere={!deviceOf(version).here}
            active={version.id === activeId}
            selected={version.id === selectedId}
            landed={version.id === landedId}
            onSelect={onSelect}
            now={now}
          />
        ))}
      </span>
      {tagged.length > 0 && (
        <span
          className={cn(
            'relative ml-3.5 flex h-5 min-w-0 items-center gap-1.5 rounded-md px-2 text-xs font-bold',
            side ? 'bg-el text-tx2' : 'bg-acc-soft text-acc',
          )}
        >
          <Diamond />
          <span className="truncate">{tagged[tagged.length - 1].tag}</span>
          {tagged.length > 1 && <span className="shrink-0">+{tagged.length - 1}</span>}
        </span>
      )}
    </motion.div>
  );
}

function Diamond({ size = 6 }: { size?: number }): ReactElement {
  return <span aria-hidden="true" className="shrink-0 rotate-45 bg-current" style={{ width: size, height: size }} />;
}

function Dot({ game, version, position, side, elsewhere, active, selected, landed, onSelect, now }: {
  game: FullGame;
  version: Version;
  position: number;
  side: boolean;
  elsewhere: boolean;
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
  const fill = version.retired ? 'transparent' : side ? 'var(--tx3)' : elsewhere ? 'var(--deck)' : 'var(--acc)';
  const left = `calc(11px + ${position.toString()} * (100% - 22px))`;

  return (
    <>
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
        className="group absolute top-1/2 z-[2] -mt-[11px] -ml-[11px] flex size-[22px] items-center justify-center rounded-full"
        style={{ left }}
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
        <span aria-hidden="true" className="pointer-events-none absolute bottom-full mb-1.5 whitespace-nowrap rounded-lg bg-tx px-2 py-1 font-mono text-xs font-semibold text-bg opacity-0 transition-opacity duration-150 group-hover:opacity-100">
          {formatTime(date, locale)} · {name}
        </span>
      </motion.button>
      {active && (
        <motion.span
          layoutId={`here-${game.id.toString()}`}
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 z-[3] -mt-[15px] -ml-[15px] size-[30px]"
          style={{ left }}
          transition={{ duration: 0.64, ease }}
        >
          <span className="absolute inset-0 rounded-xl border-2 border-acc [animation:halo_2.4s_ease-out_infinite]" />
          <span className="absolute inset-0 rounded-xl border-2 border-acc [animation:halo_2.4s_ease-out_1.2s_infinite]" />
          <GameIcon
            title={game.title}
            hue={game.accent_hue}
            iconPath={game.icon_path ?? undefined}
            size={30}
            className="relative shadow-[0_0_0_3px_var(--bg),0_6px_18px_oklch(0%_0_0/0.5)] [&]:rounded-[10px] text-[11px]"
          />
          {landed && <span className="absolute inset-0 rounded-full border-2 border-acc [animation:burst_760ms_var(--ez)_both]" />}
        </motion.span>
      )}
    </>
  );
}

function ForkView(props: SessionListProps & { fork: Fork; session: Version[]; open: boolean }): ReactElement {
  const { fork, session, open, onToggleFork, deviceOf, versions, gapMs } = props;
  const t = useT();
  const devices = fork.line.map(deviceOf);
  const other = devices.every((d) => !d.here) ? devices[0] : null;
  const label = other !== null ? t.sessions.deviceTimeline(other.name ?? t.sessions.anotherDevice) : t.sessions.anotherTimeline;
  const played = playedMs(fork.line, versions, gapMs);
  const text = [label, t.sessions.sessionCount(fork.sessions.length), ...(played >= 60_000 ? [formatDuration(played, t)] : [])].join(' · ');
  // The chip sits under the checkpoint the timeline leaves from.
  const at = fork.from === null ? 0 : trackPosition(fork.from, session);
  const forkX = 14 + TRACK_LEFT + 11 + at * (trackWidth(timeOf(session[session.length - 1]) - timeOf(session[0])) - 22);
  const chipX = Math.max(14, Math.min(300, forkX - 15));

  return (
    <motion.div layout="position" transition={{ duration: 0.48, ease }} className="relative pb-1.5">
      <span aria-hidden="true" className="absolute -top-[22px] h-[36px] border-l-[1.5px] border-dashed border-tx3" style={{ left: forkX }} />
      <motion.button
        type="button"
        aria-expanded={open}
        onClick={() => { onToggleFork(fork.root.id); }}
        initial={{ scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={spring}
        className={cn(
          'relative flex h-7 max-w-[calc(100%-28px)] items-center gap-[7px] rounded-full bg-el pl-[9px] pr-2.5 text-[12.5px] font-semibold transition-colors duration-200 hover:bg-ln',
          other !== null ? 'text-deck' : 'text-tx2',
        )}
        style={{ marginLeft: chipX }}
      >
        <CornerDownRight size={14} strokeWidth={2.4} aria-hidden="true" className="shrink-0" />
        {other !== null && <DeviceGlyph kind={other.kind} size={15} />}
        <span className="truncate">{text}</span>
        <ChevronDown size={14} strokeWidth={2.4} aria-hidden="true" className="shrink-0 transition-transform duration-300 ease-spring" style={{ rotate: open ? '180deg' : '0deg' }} />
      </motion.button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            key="drawer"
            role="group"
            aria-label={label}
            initial={{ opacity: 0, y: 10, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, scale: 0.97 }}
            transition={{ duration: 0.38, ease }}
            className="mt-1.5 rounded-2xl border-[1.5px] border-dashed border-ln bg-black/20 py-1.5"
            style={{ marginLeft: DRAWER_INDENT - 14 }}
          >
            {[...fork.sessions].reverse().map((list) => (
              <SessionRowView
                key={list[0].id}
                {...props}
                session={list}
                durationMs={timeOf(list[list.length - 1]) - timeOf(list[0])}
                side
              />
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}

function GhostRow({ versions, device, onContinue, busy, now }: {
  versions: Version[];
  device: CheckpointDevice;
  onContinue: () => void;
  busy: boolean;
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
        disabled={busy}
        onClick={onContinue}
        className="ml-auto h-[38px] shrink-0 rounded-xl bg-deck px-[18px] text-sm font-bold text-[oklch(20%_0.05_205)] transition-[transform,filter] duration-[180ms] ease-spring hover:-translate-y-px hover:brightness-110 active:scale-95 disabled:opacity-60"
      >
        {t.sessions.continueHere}
      </button>
    </motion.div>
  );
}

function EmptyGame({ title }: { title: string }): ReactElement {
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

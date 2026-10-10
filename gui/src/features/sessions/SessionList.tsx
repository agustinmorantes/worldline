// The Sessions view's main column: your current line grouped into play sessions under day
// headers, and other timelines folded into chips at their fork points.
import { type ReactElement, useState } from 'react';
import { AnimatePresence, motion, useIsPresent } from 'motion/react';
import { ChevronDown, CornerDownRight } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatTime, useLocale, useT } from '@/shared/i18n';
import { ease, spring, useMotionReduced } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { byDay, dayLabel, type Fork, playedMs, type SessionsLayout, timeOf, trackPosition, trackWidth } from './sessions';
import { Diamond, Dot, formatDuration, PendingDot } from './Timeline';

/** Time column + device column, before a track starts. */
const TRACK_LEFT = 96;
/** Extra indent of the rows inside an open fork. */
const DRAWER_INDENT = 34;
const DAY_HEADER = 'flex items-end text-[11px] font-bold uppercase tracking-[0.12em] text-tx3';

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
  /** Where the last rewind landed, for the burst. */
  landedId: number | null;
  /** The saves changed since the active checkpoint and wait to be recorded. */
  unrecorded: { dueAt: Date } | null;
  /** The game is running here. */
  running: boolean;
  now: Date;
}

export function SessionList(props: SessionListProps): ReactElement {
  const { layout, openForks, now } = props;
  const t = useT();
  const locale = useLocale();
  // Re-render as the view leaves, so the dots measure where they were before flying to the
  // Graph view: the AnimatePresence below would otherwise shield them from the change.
  useIsPresent();

  return (
    <AnimatePresence initial={false}>
      {layout.days.map((day) => (
        <section key={day.day} aria-label={dayLabel(new Date(day.day), now, locale, t.sessions)}>
          <h3 className={cn(DAY_HEADER, 'h-8 pb-2 pl-3.5')}>
            {dayLabel(new Date(day.day), now, locale, t.sessions)}
          </h3>
          {day.rows.map((row) => (
            <div key={row.versions[0].id} className="pb-1">
              <SessionRowView {...props} session={row.versions} durationMs={row.durationMs} side={false} />
              {row.forks.map((fork, i) => (
                <ForkView
                  key={fork.root.id}
                  {...props}
                  fork={fork}
                  session={row.versions}
                  open={openForks.has(fork.root.id)}
                  first={i === 0}
                  later={row.forks.slice(i + 1)}
                />
              ))}
            </div>
          ))}
        </section>
      ))}
    </AnimatePresence>
  );
}

function SessionRowView({ game, session, durationMs, side, activeId, selectedId, onSelect, deviceOf, landedId, unrecorded, running, now }: SessionListProps & {
  session: Version[];
  durationMs: number;
  /** A row of another timeline, inside its open fork. */
  side: boolean;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const device = deviceOf(session[0]);
  const elsewhere = !device.here && session.every((v) => !deviceOf(v).here);
  const playedOn = elsewhere ? t.sessions.playedOn(device.name ?? t.sessions.anotherDevice) : undefined;
  const tagged = session.filter((v) => v.tag !== null && v.tag !== '');
  const width = trackWidth(durationMs);
  const holdsSelection = selectedId !== null && session.some((v) => v.id === selectedId);
  const pending = !side && unrecorded !== null && session.some((v) => v.id === activeId) ? unrecorded : null;

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
      <Tooltip content={playedOn}>
        <span role={elsewhere ? 'img' : undefined} aria-label={playedOn} className="relative flex w-6 shrink-0 text-deck">
          {elsewhere && <DeviceGlyph kind={device.kind} size={17} />}
        </span>
      </Tooltip>
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
            left={`calc(11px + ${trackPosition(version, session).toString()} * (100% - 22px))`}
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
      <AnimatePresence>
        {pending !== null && <PendingDot key="pending" dueAt={pending.dueAt} running={running} now={now} />}
      </AnimatePresence>
      {tagged.length > 0 && (
        <span
          // The naming card's tile flies here (`NamingHost`).
          data-versions={session.map((v) => v.id).join(' ')}
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

/** Where a timeline leaves `session`'s track: x of the checkpoint it leaves from. */
function forkX(fork: Fork, session: Version[]): number {
  const at = fork.from === null ? 0 : trackPosition(fork.from, session);
  return 14 + TRACK_LEFT + 11 + at * (trackWidth(timeOf(session[session.length - 1]) - timeOf(session[0])) - 22);
}

function ForkView(props: SessionListProps & {
  fork: Fork;
  session: Version[];
  open: boolean;
  /** The first chip under its row. */
  first: boolean;
  /** The chips below this one in the row, whose lines pass behind it. */
  later: Fork[];
}): ReactElement {
  const { fork, session, open, first, later, onToggleFork, deviceOf, versions, gapMs, now } = props;
  const t = useT();
  const locale = useLocale();
  const reduced = useMotionReduced();
  const fold = reduced ? { duration: 0 } : { duration: 0.38, ease };
  const [folding, setFolding] = useState(false);
  const devices = fork.line.map(deviceOf);
  const other = devices.every((d) => !d.here) ? devices[0] : null;
  const label = other !== null
    ? t.sessions.deviceTimeline(other.name ?? t.sessions.anotherDevice)
    : fork.latest ? t.sessions.leftOff : t.sessions.anotherTimeline;
  const played = playedMs(fork.line, versions, gapMs);
  const lastPlayed = dayLabel(new Date(timeOf(fork.line[fork.line.length - 1])), now, locale, {
    today: t.sessions.today.toLocaleLowerCase(locale),
    yesterday: t.sessions.yesterday.toLocaleLowerCase(locale),
  });
  const text = [label, lastPlayed, t.sessions.sessionCount(fork.sessions.length), ...(played >= 60_000 ? [formatDuration(played, t)] : [])].join(' · ');
  // The chip sits under the checkpoint the timeline leaves from.
  const x = forkX(fork, session);
  const chipX = Math.max(14, Math.min(300, x - 15));

  return (
    <motion.div layout="position" transition={{ duration: 0.48, ease }} className="relative pb-1.5">
      {/* Each chip's line runs up to its checkpoint: the lines of the chips below pass behind this one and its drawer, blurred. */}
      {[fork, ...later].map((f) => (
        <span
          key={f.root.id}
          aria-hidden="true"
          className="absolute border-l-[1.5px] border-dashed border-tx3"
          style={{ left: forkX(f, session), top: first ? -22 : 0, bottom: f === fork ? 'calc(100% - 14px)' : 0 }}
        />
      ))}
      <motion.button
        type="button"
        aria-expanded={open}
        onClick={() => { onToggleFork(fork.root.id); }}
        initial={{ scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={spring}
        className={cn(
          'relative flex h-7 max-w-[calc(100%-28px)] items-center gap-[7px] rounded-full pl-[9px] pr-2.5 text-[12.5px] font-semibold transition-colors duration-200',
          fork.latest ? 'bg-acc-soft text-acc hover:bg-acc/20' : cn('bg-el hover:bg-ln', other !== null ? 'text-deck' : 'text-tx2'),
        )}
        style={{ marginLeft: chipX }}
      >
        <CornerDownRight size={14} strokeWidth={2.4} aria-hidden="true" className="shrink-0" />
        {other !== null && <DeviceGlyph kind={other.kind} size={15} />}
        <AnimatePresence initial={false} mode="popLayout">
          <motion.span
            key={label}
            className="truncate"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={reduced ? { duration: 0 } : { duration: 0.2 }}
          >
            {text}
          </motion.span>
        </AnimatePresence>
        <ChevronDown size={14} strokeWidth={2.4} aria-hidden="true" className="shrink-0 transition-transform duration-300 ease-spring" style={{ rotate: open ? '180deg' : '0deg' }} />
      </motion.button>
      {/* Clips only while folding (closing counts), so the selection and dots flying in aren't cut
          off. On this wrapper, not the fold: a leaving fold keeps its last props. */}
      <div className={cn((folding || !open) && 'overflow-hidden')}>
        <AnimatePresence initial={false}>
          {open && (
            // Folds open, so the chips below, and the lines passing behind, move with it. The fade
            // sits on the drawer itself: one on the fold would keep its blur off until the end.
            <motion.div
              key="drawer"
              initial={{ height: 0 }}
              animate={{ height: 'auto' }}
              exit={{ height: 0 }}
              transition={fold}
              onAnimationStart={() => { setFolding(true); }}
              onAnimationComplete={() => { setFolding(false); }}
            >
              <motion.div
                role="group"
                aria-label={label}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={fold}
                className="mt-1.5 rounded-2xl border-[1.5px] border-dashed border-ln bg-black/20 py-1.5 backdrop-blur-[2px]"
                style={{ marginLeft: DRAWER_INDENT - 14 }}
              >
                {byDay([...fork.sessions].reverse(), (list) => list[0]).map((day) => (
                  <section key={day.day} aria-label={dayLabel(new Date(day.day), now, locale, t.sessions)}>
                    <h3 className={cn(DAY_HEADER, 'h-7 pb-1.5 pl-12')}>{dayLabel(new Date(day.day), now, locale, t.sessions)}</h3>
                    {day.rows.map((list) => (
                      <SessionRowView
                        key={list[0].id}
                        {...props}
                        session={list}
                        durationMs={timeOf(list[list.length - 1]) - timeOf(list[0])}
                        side
                      />
                    ))}
                  </section>
                ))}
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}

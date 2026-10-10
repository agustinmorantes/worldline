// The Sessions view's main column: your current line grouped into play sessions under day
// headers, and other timelines folded into chips at their fork points.
import type { ReactElement } from 'react';
import { AnimatePresence, motion, useIsPresent } from 'motion/react';
import { ChevronDown, CornerDownRight } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatTime, useLocale, useT } from '@/shared/i18n';
import { ease, spring } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { dayLabel, type Fork, playedMs, type SessionsLayout, timeOf, trackPosition, trackWidth } from './sessions';
import { Diamond, Dot, formatDuration } from './Timeline';

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
  /** Where the last rewind landed, for the burst. */
  landedId: number | null;
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
  );
}

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
  const playedOn = elsewhere ? t.sessions.playedOn(device.name ?? t.sessions.anotherDevice) : undefined;
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

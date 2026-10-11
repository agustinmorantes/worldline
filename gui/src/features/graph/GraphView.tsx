// The Graph view: every checkpoint of every timeline on one vertical time axis, newest at the
// top. Your line is the accent spine; other timelines branch into dimmed lanes; names line up in
// one column. Colors follow `markColor`. Dots share their layout ids with the Sessions view, so they fly between the two.
import type { ReactElement } from 'react';
import { AnimatePresence, motion, useIsPresent } from 'motion/react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import type { CheckpointDevice } from '@/features/game/game-view';
import { dayLabel, timeOf } from '@/features/sessions/sessions';
import { Diamond, Dot, formatDuration, markColor, tint } from '@/features/sessions/Timeline';
import { formatTime, useLocale, useT } from '@/shared/i18n';
import { ease } from '@/shared/motion/motion';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import type { GraphEdge, GraphLayout } from './graph';

/** Furniture fades in once the dots have started moving. */
const appear = { initial: { opacity: 0 }, animate: { opacity: 1 }, transition: { duration: 0.32, delay: 0.26 } };

export function GraphView({ game, graph, versions, activeId, selectedId, onSelect, deviceOf, landedId, now }: {
  game: FullGame;
  graph: GraphLayout;
  versions: Version[];
  activeId: number | null;
  selectedId: number | null;
  onSelect: (id: number) => void;
  deviceOf: (version: Version) => CheckpointDevice;
  landedId: number | null;
  now: Date;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const elsewhere = (list: Version[]): boolean => list.every((v) => !deviceOf(v).here);
  const selected = selectedId === null ? undefined : graph.points.get(selectedId);
  // As in the Sessions view: re-render as the view leaves, so the dots measure where they were.
  useIsPresent();

  return (
    <div className="relative" style={{ height: graph.height }}>
      {selected !== undefined && (
        <motion.span
          layoutId={`selection-${game.id.toString()}`}
          aria-hidden="true"
          className="pointer-events-none absolute right-1 h-[26px] rounded-[14px] bg-acc-soft shadow-[inset_0_0_0_1px_var(--acc-line)]"
          style={{ top: selected.y - 13, left: 98 }}
          transition={{ duration: 0.38, ease }}
        />
      )}

      {graph.days.map((day) => (
        <motion.h3 key={day.day} {...appear} className="absolute left-3.5 pt-2.5 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3" style={{ top: day.y }}>
          {dayLabel(new Date(day.day), now, locale, t.sessions)}
        </motion.h3>
      ))}

      {graph.clusters.map((cluster) => {
        const newest = cluster.versions[0];
        const oldest = cluster.versions[cluster.versions.length - 1];
        const deck = elsewhere(cluster.versions);
        const device = deviceOf(oldest);
        const playedOn = t.sessions.playedOn(device.name ?? t.sessions.anotherDevice);
        return (
          <motion.div key={newest.id} {...appear} className="absolute left-3.5 flex flex-col gap-[3px] font-mono" style={{ top: cluster.y }}>
            <span className={cn('flex items-center gap-1.5 text-[13px] font-semibold leading-4', cluster.versions.some((v) => graph.points.get(v.id)?.lane === 0) ? 'text-tx' : 'text-tx3')}>
              {formatTime(new Date(timeOf(oldest)), locale)}
              {deck && (
                <Tooltip content={playedOn}>
                  <span role="img" aria-label={playedOn} className="flex text-deck">
                    <DeviceGlyph kind={device.kind} size={14} />
                  </span>
                </Tooltip>
              )}
            </span>
            <span className="text-[11.5px] leading-[14px] text-tx3">{formatDuration(timeOf(newest) - timeOf(oldest), t)}</span>
          </motion.div>
        );
      })}

      {graph.capsules.map((capsule) => {
        const side = capsule.lane > 0;
        const color = markColor({ side, elsewhere: elsewhere(capsule.versions) });
        return (
          <motion.span
            key={capsule.versions[0].id}
            {...appear}
            aria-hidden="true"
            className="absolute w-3.5 rounded-full"
            style={{
              left: capsule.x,
              top: capsule.y,
              height: capsule.height,
              backgroundColor: tint(color, side ? 8 : 20),
              boxShadow: side ? `inset 0 0 0 1.5px ${tint(color, 50)}` : undefined,
            }}
          />
        );
      })}

      <svg aria-hidden="true" className="pointer-events-none absolute inset-0 size-full overflow-visible">
        {graph.edges.map((edge, i) => {
          const id = `edge-${game.id.toString()}-${edge.versions[0].id.toString()}`;
          const stops = edgeStops(edge, graph, (v) => !deviceOf(v).here);
          return (
            <g key={edge.versions[0].id}>
              {stops.length > 1 && (
                <linearGradient id={id} gradientUnits="userSpaceOnUse" x1={0} x2={0} y1={stops[0].y} y2={stops[stops.length - 1].y}>
                  {stops.map((stop) => (
                    <stop key={`${stop.y.toString()} ${stop.color}`} offset={(stop.y - stops[0].y) / Math.max(1, stops[stops.length - 1].y - stops[0].y)} style={{ stopColor: stop.color }} />
                  ))}
                </linearGradient>
              )}
              <motion.path
                d={edge.d}
                fill="none"
                strokeLinecap="round"
                stroke={stops.length > 1 ? `url(#${id})` : stops[0].color}
                strokeWidth={edge.lane === 0 ? 3 : 2}
                initial={{ pathLength: 0 }}
                animate={{ pathLength: 1 }}
                transition={{ duration: 0.7, ease, delay: edge.lane === 0 ? 0.18 : 0.34 + i * 0.09 }}
              />
            </g>
          );
        })}
      </svg>

      {graph.labels.map((label) => {
        const color = markColor({ side: label.lane > 0, elsewhere: !deviceOf(label.version).here });
        return (
          <motion.div key={label.version.id} {...appear}>
            <span
              aria-hidden="true"
              className="absolute h-0 border-t-[1.5px] border-dotted"
              style={{ left: label.from, top: label.y, width: Math.max(0, graph.labelX - label.from - 4), borderColor: tint(color, 45) }}
            />
            <button
              type="button"
              tabIndex={-1}
              onClick={() => { onSelect(label.version.id); }}
              className="absolute flex h-5 items-center gap-1.5 overflow-hidden whitespace-nowrap rounded-md px-2 text-xs font-bold hover:brightness-125"
              style={{ left: graph.labelX, top: label.y - 10, maxWidth: `calc(100% - ${(graph.labelX + 12).toString()}px)`, color, backgroundColor: tint(color, 14) }}
            >
              <Diamond />
              <span className="truncate">{label.version.tag}</span>
            </button>
          </motion.div>
        );
      })}

      {/* Dots that fly in from the other view must not also pop in from nothing. */}
      <AnimatePresence initial={false}>
        {versions.map((version) => {
          const point = graph.points.get(version.id);
          if (point === undefined) return null;
          return (
            <Dot
              key={version.id}
              game={game}
              version={version}
              left={point.x}
              top={point.y}
              side={point.lane > 0}
              elsewhere={!deviceOf(version).here}
              active={version.id === activeId}
              selected={version.id === selectedId}
              landed={version.id === landedId}
              onSelect={onSelect}
              now={now}
            />
          );
        })}
      </AnimatePresence>
    </div>
  );
}

/**
 * Where a line changes color, top to bottom: each stretch takes the color of the checkpoint at
 * its top, the one that play led to, and the stretch from where the line leaves its parent takes
 * its oldest checkpoint's. Past the last stop the gradient keeps its color. One stop when the
 * whole line is one color.
 */
function edgeStops(edge: GraphEdge, graph: GraphLayout, elsewhere: (version: Version) => boolean): { y: number; color: string }[] {
  // The spine lists your line oldest first; other lines newest first.
  const newest = edge.lane === 0 ? [...edge.versions].reverse() : edge.versions;
  const marks = newest.map((v) => ({ y: graph.points.get(v.id)?.y ?? 0, color: markColor({ side: edge.lane > 0, elsewhere: elsewhere(v) }) }));
  const stops = [marks[0]];
  for (let k = 1; k < marks.length; k++) {
    if (marks[k].color === marks[k - 1].color) continue;
    stops.push({ y: marks[k].y, color: marks[k - 1].color }, marks[k]);
  }
  return stops;
}

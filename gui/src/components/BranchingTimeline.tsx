import { ReactElement, useState, useMemo } from 'react';
import { Version } from '@/models/version';
import { formatDistanceToNow } from 'date-fns';
import { cn } from '@/lib/utils';
import { Button } from './ui/button';
import { RotateCcw, Tag as TagIcon, CheckCircle2 } from 'lucide-react';
import { Badge } from './ui/badge';

interface BranchingTimelineProps {
  versions: Version[];
  activeVersionId?: number;
  accentHue?: number;
  showActions?: boolean;
  compact?: boolean;
  onRestore?: (version: Version) => void;
  onTag?: (version: Version, tag: string) => void;
  remoteVersions?: Partial<Record<string, string>>;
}

const NO_REMOTE_VERSIONS: Partial<Record<string, string>> = {};

function provenance(version: Version, remoteVersions: Partial<Record<string, string>>): string {
  const device = version.uuid === undefined ? undefined : remoteVersions[version.uuid];
  return device === undefined ? 'Created on this device' : `Synced from ${device}`;
}

export function BranchingTimeline({
  versions,
  activeVersionId,
  accentHue = 255,
  showActions = true,
  compact = false,
  onRestore,
  onTag,
  remoteVersions = NO_REMOTE_VERSIONS,
}: BranchingTimelineProps): ReactElement {
  const [editingId, setEditingId] = useState<number | null>(null);
  const canEditTags = showActions;
  const metrics = useMemo(() => ({
    rowHeight: compact ? 56 : 70,
    laneWidth: compact ? 28 : 34,
    nodeRadius: compact ? 6 : 7,
    leftPad: compact ? 18 : 24,
    svgPad: compact ? 14 : 20,
    topPad: compact ? 6 : 10,
    bottomPad: compact ? 8 : 10,
  }), [compact]);

  // 1. Build the tree and assign lanes
  const { processedVersions, lanesCount } = useMemo(() => {
    if (versions.length === 0) return { processedVersions: [], lanesCount: 0 };

    const versionMap = new Map(versions.map(v => [v.id, v]));

    // Walk up from active version via parent_id to build the ancestor set
    const ancestors = new Set<number>();
    let curr = activeVersionId;
    while (curr !== undefined && versionMap.has(curr)) {
      ancestors.add(curr);
      curr = versionMap.get(curr)?.parent_id ?? undefined;
    }

    // Pre-branching versions have parent_id = null/undefined (created before tree tracking).
    // They form a linear chain by version_number. Any such version at or before the
    // earliest null-parent ancestor is also live history, not an inactive branch.
    const nullParentAncestors = [...ancestors].filter(
      id => versionMap.get(id)?.parent_id === undefined
    );
    if (nullParentAncestors.length > 0) {
      const lowestVn = Math.min(
        ...nullParentAncestors.map(id => versionMap.get(id)?.version_number ?? Infinity)
      );
      for (const v of versions) {
        if (v.parent_id === undefined && v.id !== undefined && v.version_number <= lowestVn) {
          ancestors.add(v.id);
        }
      }
    }

    const laneMap = new Map<number, number>();
    let nextLane = 1;

    // Sort versions by date descending (newest at top).
    // Active version is NOT forced to top — it appears at its correct temporal position,
    // which keeps parent→child edges always pointing downward in the layout.
    const sortedVersions = [...versions].sort((a, b) => {
      return (b.created_at?.getTime() ?? 0) - (a.created_at?.getTime() ?? 0);
    });

    // 1. Assign Lane 0 to the main path (ancestors of the active version)
    sortedVersions.forEach(v => {
      const vid = v.id;
      if (vid !== undefined && ancestors.has(vid)) {
        laneMap.set(vid, 0);
      }
    });

    // 2. Assign other versions to lanes greedily.
    // We iterate from newest to oldest. For each unassigned version, we start a new lane
    // and follow its parent chain down, assigning all unassigned parents to that same lane.
    sortedVersions.forEach(v => {
      const vid = v.id;
      if (vid === undefined || laneMap.has(vid)) return;

      const lane = nextLane++;
      let currId: number | undefined = vid;
      
      while (currId !== undefined && !laneMap.has(currId)) {
        laneMap.set(currId, lane);
        currId = versionMap.get(currId)?.parent_id ?? undefined;
      }
    });

    const processed = sortedVersions.map((v, index) => {
      const vid = v.id ?? 0;
      return {
        version: v,
        lane: laneMap.get(vid) ?? 0,
        // A version is only inactive when there IS an active version and this version is not on its path
        isInactive: activeVersionId !== undefined && !ancestors.has(vid) && vid !== activeVersionId,
        isActive: vid === activeVersionId,
        y: metrics.topPad + index * metrics.rowHeight + metrics.rowHeight / 2,
      };
    });

    return { 
      processedVersions: processed, 
      lanesCount: Math.max(0, ...Array.from(laneMap.values())) + 1 
    };
  }, [versions, activeVersionId, metrics.rowHeight, metrics.topPad]);

  const BRANCH_COLORS = useMemo(() => [
    `oklch(64% 0.22 ${accentHue.toString()}deg)`,
    `oklch(64% 0.22 ${((accentHue + 155) % 360).toString()}deg)`,
    `oklch(64% 0.22 ${((accentHue + 55) % 360).toString()}deg)`,
    `oklch(64% 0.22 ${((accentHue + 215) % 360).toString()}deg)`,
  ], [accentHue]);

  const getX = (lane: number): number => metrics.leftPad + lane * metrics.laneWidth;
  const totalHeight = metrics.topPad + processedVersions.length * metrics.rowHeight + metrics.bottomPad;
  const totalWidth = metrics.leftPad + lanesCount * metrics.laneWidth + metrics.svgPad;

  return (
    <div className="relative flex">
      {/* SVG Background for connections */}
      <svg 
        className="absolute top-0 left-0 pointer-events-none" 
        width={totalWidth}
        height={totalHeight}
      >
        {processedVersions.map((pv) => {
          const parentId = pv.version.parent_id;
          if (parentId === undefined) return null;

          const parentPv = processedVersions.find(p => p.version.id === parentId);
          if (!parentPv) return null;

          const x1 = getX(pv.lane);
          const y1 = pv.y;
          const x2 = getX(parentPv.lane);
          const y2 = parentPv.y;

          const isInactive = pv.isInactive || parentPv.isInactive;
          const color = BRANCH_COLORS[pv.lane % BRANCH_COLORS.length] ?? BRANCH_COLORS[0];

          if (pv.lane === parentPv.lane) {
            // Straight line
            return (
              <line
                key={`edge-${pv.version.id?.toString() ?? 'unknown'}`}
                x1={x1} y1={y1} x2={x2} y2={y2}
                stroke={color}
                strokeWidth={2}
                strokeDasharray={isInactive ? "4 4" : "0"}
                opacity={isInactive ? 0.45 : 1}
              />
            );
          } else {
            // Bézier curve for branching
            const cpY = y1 + (y2 - y1) * 0.5;
            return (
              <path
                key={`edge-${pv.version.id?.toString() ?? 'unknown'}`}
                d={`M ${x1.toString()} ${y1.toString()} C ${x1.toString()} ${cpY.toString()}, ${x2.toString()} ${cpY.toString()}, ${x2.toString()} ${y2.toString()}`}
                fill="none"
                stroke={color}
                strokeWidth={2}
                strokeDasharray={isInactive ? "4 4" : "0"}
                opacity={isInactive ? 0.45 : 1}
              />
            );
          }
        })}
      </svg>

      {/* Rows Container */}
      <div className="flex-1">
        {processedVersions.map((pv) => (
          <div 
            key={pv.version.id} 
            className="flex items-center group relative" 
            style={{ height: metrics.rowHeight }}
            title={provenance(pv.version, remoteVersions)}
          >
            {/* Node Area */}
            <div
              style={{ width: totalWidth }}
              className="relative h-full flex-shrink-0"
            >
              <div 
                className="absolute" 
                style={{ 
                  left: getX(pv.lane) - metrics.nodeRadius,
                  top: '50%', 
                  transform: 'translateY(-50%)' 
                }}
              >
                {/* Pulse ring for active */}
                {pv.isActive && (
                  <div 
                    className="absolute inset-0 rounded-full animate-[pulseRing_2s_infinite]"
                    style={{ backgroundColor: BRANCH_COLORS[pv.lane % BRANCH_COLORS.length] ?? BRANCH_COLORS[0] }}
                  />
                )}
                
                {/* Main Node */}
                <div 
                  className={cn(
                    compact ? "relative z-10 h-3 w-3 rounded-full border-2 bg-sf" : "relative z-10 h-3.5 w-3.5 rounded-full border-2 bg-sf",
                    pv.isActive ? "bg-white" : ""
                  )}
                  style={{ 
                    borderColor: BRANCH_COLORS[pv.lane % BRANCH_COLORS.length] ?? BRANCH_COLORS[0],
                    opacity: pv.isInactive ? 0.45 : 1,
                    borderStyle: pv.isInactive ? 'dashed' : 'solid'
                  }}
                >
                  {pv.isActive && (
                    <div className="absolute inset-0.5 rounded-full bg-white" />
                  )}
                </div>
              </div>
            </div>

            {/* Content Area */}
            <div className={cn("flex min-w-0 flex-1 items-center justify-between", compact ? "pr-3" : "pr-6")}>
              <div className="flex flex-col min-w-0">
                <div className="flex items-center gap-2">
                  <span 
                    className={cn("font-mono font-bold", compact ? "text-[11px]" : "text-xs")}
                    style={{ color: BRANCH_COLORS[pv.lane % BRANCH_COLORS.length] ?? BRANCH_COLORS[0] }}
                  >
                    v{pv.version.version_number.toString().padStart(3, '0')}
                  </span>
                  
                  {editingId === pv.version.id ? (
                    <input
                      autoFocus
                      className="text-sm bg-el border border-primary/40 rounded px-1.5 py-0.5 outline-none text-foreground w-48 font-medium"
                      defaultValue={pv.version.tag ?? ''}
                      onBlur={(e) => {
                        onTag?.(pv.version, e.target.value);
                        setEditingId(null);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') {
                          onTag?.(pv.version, e.currentTarget.value);
                          setEditingId(null);
                        }
                        if (e.key === 'Escape') setEditingId(null);
                      }}
                    />
                  ) : pv.version.tag !== undefined && pv.version.tag !== '' ? (
                    <span 
                      className={cn(
                        "truncate font-medium text-foreground transition-colors",
                        compact ? "text-[13px]" : "text-sm",
                        canEditTags ? "cursor-pointer hover:text-primary" : ""
                      )}
                      onClick={() => {
                        if (canEditTags && pv.version.id !== undefined) {
                          setEditingId(pv.version.id);
                        }
                      }}
                    >
                      {pv.version.tag}
                    </span>
                  ) : (
                    <span
                      className={cn(
                        "italic text-muted-foreground/40 transition-colors",
                        compact ? "text-[13px]" : "text-sm",
                        canEditTags ? "cursor-pointer hover:text-muted-foreground/60" : ""
                      )}
                      onClick={() => {
                        if (canEditTags && pv.version.id !== undefined) {
                          setEditingId(pv.version.id);
                        }
                      }}
                    >
                      No description
                    </span>
                  )}

                  {pv.isActive && (
                    <span className={cn(
                      "flex items-center gap-1 rounded border border-primary/20 bg-primary/10 font-bold text-primary",
                      compact ? "px-1.5 py-0.5 text-[9px]" : "px-1.5 py-0.5 text-[10px]"
                    )}>
                      <CheckCircle2 size={10} /> CURRENT
                    </span>
                  )}
                  {pv.version.uuid !== undefined && remoteVersions[pv.version.uuid] !== undefined ? (
                    <Badge variant="secondary">
                      {remoteVersions[pv.version.uuid]}
                    </Badge>
                  ) : null}
                </div>
                
                <span className={cn("mt-0.5 font-medium text-muted-foreground", compact ? "text-[9px]" : "text-[10px]")}>
                  {pv.version.created_at ? formatDistanceToNow(pv.version.created_at, { addSuffix: true }) : 'unknown time'}
                </span>
              </div>

              {/* Actions */}
              {showActions && (
              <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
                <Button 
                  variant="ghost" 
                  size="sm" 
                  className="h-8 w-8 p-0"
                  onClick={() => { onTag?.(pv.version, pv.version.tag ?? ''); }}
                >
                  <TagIcon size={14} className="text-muted-foreground" />
                </Button>
                
                <Button 
                  variant="secondary" 
                  size="sm" 
                  className="h-8 gap-1.5 text-xs font-semibold"
                  onClick={() => { onRestore?.(pv.version); }}
                >
                  <RotateCcw size={12} /> Restore
                </Button>
              </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

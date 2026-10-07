import { ReactElement, ReactNode } from 'react';
import { Activity, RadioTower } from 'lucide-react';

interface EmptyTimelineStateProps {
  gameTitle: string;
  trackedDirCount: number;
}

function formatTrackedDirLabel(count: number): string {
  return `${count.toString()} monitored ${count === 1 ? 'folder' : 'folders'}`;
}

export function EmptyTimelineState({
  gameTitle,
  trackedDirCount,
}: EmptyTimelineStateProps): ReactElement {
  const isMonitoring = trackedDirCount > 0;

  return (
    <div className="rounded-[28px] border border-dashed border-border/60 bg-transparent px-6 py-7">
      <div className="flex flex-col gap-6">
        <div className="max-w-2xl">
          <div className="mb-4 inline-flex items-center gap-2 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-3 py-1 text-[10px] font-bold uppercase tracking-[0.24em] text-emerald-600 dark:text-emerald-300">
            <span className="relative flex h-2.5 w-2.5">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400/65" />
              <span className="relative inline-flex h-2.5 w-2.5 rounded-full bg-emerald-500" />
            </span>
            {isMonitoring ? 'Monitoring for save changes' : 'Monitoring not started'}
          </div>

          <h2 className="text-2xl font-black tracking-tight text-foreground">
            {isMonitoring ? `Worldline is watching ${gameTitle}` : `Set up monitoring for ${gameTitle}`}
          </h2>

          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
            {isMonitoring
              ? `Your first version will appear here automatically after the next save change. Keep playing, save in-game, and Worldline will capture the moment for you.`
              : `This game was added successfully, but Worldline needs at least one save folder to watch before it can create versions automatically.`}
          </p>

          <div className="mt-5 flex flex-wrap gap-3">
            <StatusPill
              icon={<RadioTower size={14} />}
              label={isMonitoring ? formatTrackedDirLabel(trackedDirCount) : 'No tracked folders yet'}
            />
            <StatusPill
              icon={<Activity size={14} />}
              label={isMonitoring ? 'Waiting for the first save event' : 'Add a folder to enable automatic versions'}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

function StatusPill({ icon, label }: { icon: ReactNode; label: string }): ReactElement {
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-border/50 bg-background/70 px-3 py-1.5 text-xs font-medium text-foreground shadow-sm backdrop-blur-sm">
      <span className="text-primary">{icon}</span>
      <span>{label}</span>
    </div>
  );
}

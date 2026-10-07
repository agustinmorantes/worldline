import { ReactElement } from 'react';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { Button } from '@/shared/ui/button';
import { Info, AlertTriangle } from 'lucide-react';
import { formatDistanceToNow } from 'date-fns';
import type { Version } from '@/ipc/bindings/Version';
import { utcDate } from '@/ipc/time';

interface RestoreModalProps {
  version: Version | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function RestoreModal({ version, onConfirm, onCancel }: RestoreModalProps): ReactElement {
  return (
    <Dialog open={version !== null} onOpenChange={(open) => { if (!open) onCancel(); }}>
      <DialogContent className="sm:max-w-[420px] bg-sf border-border/40">
        <DialogHeader>
          <DialogTitle className="text-lg font-bold tracking-tight">Restore Snapshot</DialogTitle>
        </DialogHeader>

        {version !== null && (
          <div className="space-y-4 py-2">
            <div className="p-4 rounded-xl bg-el border border-border/40">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 rounded-lg bg-primary/10 border border-primary/20 flex items-center justify-center shrink-0">
                  <span className="text-xs font-mono font-bold text-primary">
                    v{version.version_number.toString().padStart(3, '0')}
                  </span>
                </div>
                <div className="min-w-0">
                  {version.tag !== null && version.tag !== '' && (
                    <p className="text-sm font-bold truncate">{version.tag}</p>
                  )}
                  <p className="text-xs text-muted-foreground">
                    {formatDistanceToNow(utcDate(version.created_at), { addSuffix: true })}
                  </p>
                </div>
              </div>
            </div>

            <div className="space-y-2">
              <div className="flex gap-3 p-3 rounded-lg bg-primary/5 border border-primary/10">
                <Info size={16} className="text-primary shrink-0 mt-0.5" />
                <p className="text-xs text-muted-foreground leading-relaxed">
                  Restoring this version will create a <span className="text-primary font-medium">new branch</span>. Previous progress is never lost and remains accessible in the timeline.
                </p>
              </div>

              <div className="flex gap-3 p-3 rounded-lg bg-warn/10 border border-warn/20">
                <AlertTriangle size={16} className="text-warn shrink-0 mt-0.5" />
                <p className="text-xs text-muted-foreground leading-relaxed">
                  <span className="text-warn font-bold">Important:</span> Please close the game before restoring to prevent file corruption or synchronization issues.
                </p>
              </div>
            </div>
          </div>
        )}

        <DialogFooter className="gap-2 sm:gap-0">
          <Button variant="ghost" onClick={onCancel}>Cancel</Button>
          <Button variant="destructive" onClick={onConfirm} className="font-bold">
            Restore Snapshot
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

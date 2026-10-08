import {toast} from "sonner";
import {getGame, getStartupNotice} from "@/ipc/commands";
import {on} from "@/ipc/events";
import {queryClient} from "@/app/query-client";
import {reportError} from "@/shared/errors/report";
import {getT} from "@/shared/i18n";
import {isFolderUnreachable, setSyncOutcome, setSyncRunning} from "@/features/library/sync-activity";

export function listenNotifications(): void {
  void on("version-created", ([gameId, versionNumber]) => {
    // Invalidate queries to refresh data
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game', gameId] });
    void queryClient.invalidateQueries({ queryKey: ['versions', gameId] });

    // The game's own view shows the new dot instead, and a hidden window shows nothing.
    if (document.hidden || window.location.pathname === `/game/${gameId.toString()}`) return;
    void getGame(gameId)
      .then(game => {
        toast("Version created", {
          description: `${game.title} - v${versionNumber.toString()}`
        })
      });
  })

  void on("detection-error", payload => {
    reportError(getT().errors.detectionFailed, payload);
  })

  void on('sync-started', () => {
    setSyncRunning(true);
  });

  void on('sync-complete', report => {
    setSyncRunning(false);
    setSyncOutcome(null);
    queryClient.setQueryData(['sync-report'], report);
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game'] });
    void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
  });

  // Background syncs retry every few minutes; an offline folder is not worth a toast each
  // time. The sync card and Settings → Cloud say it instead.
  void on('sync-error', payload => {
    setSyncRunning(false);
    setSyncOutcome(payload);
    console.warn('Sync failed:', payload);
  });

  // Cleanup bails on an unreachable folder too, every time it comes due.
  void on('pruning-error', payload => {
    if (isFolderUnreachable(payload)) return;
    reportError(getT().errors.pruningFailed, payload);
  });

  void getStartupNotice()
    .then(backup => {
      if (backup !== null) toast.info(getT().notices.oldDataMoved(backup), { duration: Infinity });
    })
    .catch((err: unknown) => { reportError("Couldn't read the startup notice", err); });

  void on('history-pruned', () => {
    void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game'] });
  });
}

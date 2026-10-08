import {toast} from "sonner";
import {getGame, getStartupNotice} from "@/ipc/commands";
import {on} from "@/ipc/events";
import {queryClient} from "@/app/query-client";
import {reportError} from "@/shared/errors/report";
import {getT} from "@/shared/i18n";

export function listenNotifications(): void {
  void on("version-created", ([gameId, versionNumber]) => {
    // Invalidate queries to refresh data
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game', gameId] });
    void queryClient.invalidateQueries({ queryKey: ['versions', gameId] });

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

  void on('sync-complete', report => {
    queryClient.setQueryData(['sync-report'], report);
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game'] });
    void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
  });

  // Background syncs retry every few minutes; an offline folder is not worth a toast each time.
  void on('sync-error', payload => {
    console.warn('Sync failed:', payload);
  });

  void on('pruning-error', payload => {
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

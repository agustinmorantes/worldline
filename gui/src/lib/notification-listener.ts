import {listen} from "@tauri-apps/api/event";
import {toast} from "sonner";
import {getGame} from "@/api/games.ts";
import {queryClient} from "@/lib/query-client.ts";
import { SyncReportSchema } from '@/models/sync';
import { getStartupNotice } from '@/api/settings';

export function listenNotifications(): void {
  void listen<number[]>("version-created", event => {
    const gameId = event.payload[0];
    const versionNumber = event.payload[1];

    // Invalidate queries to refresh data
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game', gameId] });
    void queryClient.invalidateQueries({ queryKey: ['versions', gameId] });

    void getGame(gameId)
      .then(game => {
        toast("Version created", {
          description: `${game?.title ?? ''} - v${versionNumber.toString()}`
        })
      });
  })

  void listen<string>("detection-error", event => {
    toast("Detection error", {
      description: event.payload
    });
  })

  void listen<unknown>('sync-complete', event => {
    const report = SyncReportSchema.safeParse(event.payload);
    if (report.success) {
      queryClient.setQueryData(['sync-report'], report.data);
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void queryClient.invalidateQueries({ queryKey: ['game'] });
      void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
    }
  });

  // Background syncs retry every few minutes; an offline folder is not worth a toast each time.
  void listen<string>('sync-error', event => {
    console.warn('Sync failed:', event.payload);
  });

  void listen<string>('pruning-error', event => {
    toast.error('Automatic history cleanup failed', { description: event.payload });
  });

  void getStartupNotice()
    .then(notice => {
      if (notice !== null) toast.info(notice, { duration: Infinity });
    })
    .catch((err: unknown) => { console.error(err); });

  void listen('history-pruned', () => {
    void queryClient.invalidateQueries({ queryKey: ['retired-versions'] });
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['game'] });
  });
}

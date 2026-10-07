import { ReactElement } from 'react';
import { createFileRoute } from '@tanstack/react-router';
import { Cloud, Download } from 'lucide-react';
import { useSyncReport, useSyncSettings } from '@/features/library/queries';
import { adoptRemoteGame } from '@/ipc/commands';
import { reportError } from '@/shared/errors/report';
import { queryClient } from '@/app/query-client';
import { Button } from '@/shared/ui/button';
import { Badge } from '@/shared/ui/badge';
import { ScrollArea } from '@/shared/ui/scroll-area';
import { toast } from 'sonner';

export const Route = createFileRoute('/remote')({ component: RemoteView });

function RemoteView(): ReactElement {
  const { data: settings } = useSyncSettings();
  const { data, isFetching, error, refetch } = useSyncReport();
  const syncOff = settings !== undefined && (settings.remote_path ?? '') === '';
  const remoteGames = data?.remote_games.filter(game => !game.adopted) ?? [];

  const adopt = async (uuid: string): Promise<void> => {
    try {
      await adoptRemoteGame(uuid);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['games'] }),
        refetch(),
      ]);
      toast.success('Remote game adopted');
    } catch (err) {
      reportError('Failed to adopt remote game', err);
    }
  };

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      <header className="p-8 pb-4">
        <h1 className="text-3xl font-black tracking-tight">Remote Games</h1>
        <p className="mt-1 text-sm text-muted-foreground">Progress available in your sync folder but not tracked here.</p>
      </header>
      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-8 pt-4">
        {syncOff ? (
          <div className="flex flex-col items-center gap-3 py-20 text-center text-muted-foreground">
            <Cloud />
            <p>Sync is off. Choose a sync folder in Settings to see games from your other devices.</p>
          </div>
        ) : error !== null ? (
          <div className="flex flex-col items-start gap-3 rounded-xl border border-border/40 bg-sf p-5">
            <p className="text-sm text-muted-foreground">Configure a reachable sync folder in Settings.</p>
            <Button variant="outline" onClick={() => { void refetch(); }}>Retry</Button>
          </div>
        ) : remoteGames.length === 0 ? (
          <div className="flex flex-col items-center gap-3 py-20 text-center text-muted-foreground">
            <Cloud />
            <p>{isFetching ? 'Checking the remote…' : 'Every remote game is already adopted.'}</p>
          </div>
        ) : (
          <div className="grid gap-3">
            {remoteGames.map(game => (
              <section key={game.uuid} className="flex items-center justify-between gap-4 rounded-xl border border-border/40 bg-sf p-5">
                <div>
                  <h2 className="text-sm font-bold">{game.title}</h2>
                  {game.installed ? (
                    <Badge variant="secondary">Installed</Badge>
                  ) : (
                    <p className="text-xs text-muted-foreground">Not installed on this device</p>
                  )}
                </div>
                <Button onClick={() => { void adopt(game.uuid); }}>
                  <Download data-icon="inline-start" /> Adopt
                </Button>
              </section>
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

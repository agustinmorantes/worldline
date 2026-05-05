import { createFileRoute, useRouter } from '@tanstack/react-router';
import { ReactElement, useState, useMemo } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { saveGame } from '@/api/games';
import { InstallScannerItem } from '@/models/install_scanner_item';
import { Game } from '@/models/game';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { GameIcon } from '@/components/GameIcon';
import { toast } from 'sonner';
import { CheckCircle2, Info } from 'lucide-react';
import { CheckedState } from '@radix-ui/react-checkbox';
import { useScannedGames } from '@/api/queries';

export const Route = createFileRoute('/import')({
  component: ImportGames,
});

function ImportGames(): ReactElement {
  const router = useRouter();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(() => new Set());

  const { data: scanned, isLoading, error } = useScannedGames();

  const importMutation = useMutation({
    mutationFn: async (items: InstallScannerItem[]) => {
      const results = await Promise.all(
        items.map(async (i) => {
          const game = new Game(i.title);
          game.tracked_dirs = i.dirs.map(d => ({ path: d }));
          return await saveGame(game);
        })
      );
      return results;
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      toast.success(`Successfully imported ${selected.size.toString()} games`);
      void router.navigate({ to: '/' });
    },
    onError: (err) => {
      console.error(err);
      toast.error("Failed to import games");
    }
  });

  const handleToggleAll = (state: CheckedState): void => {
    if (!scanned) return;
    if (state === false) {
      setSelected(new Set());
    } else {
      setSelected(new Set(scanned.map(i => i.title)));
    }
  };

  const handleToggleItem = (item: InstallScannerItem, state: CheckedState): void => {
    const next = new Set(selected);
    if (state === false) {
      next.delete(item.title);
    } else {
      next.add(item.title);
    }
    setSelected(next);
  };

  const handleImport = (): void => {
    if (!scanned) return;
    const toImport = scanned.filter(i => selected.has(i.title));
    importMutation.mutate(toImport);
  };

  const allChecked: CheckedState = useMemo(() => {
    if (!scanned || scanned.length === 0) return false;
    if (selected.size === 0) return false;
    if (selected.size === scanned.length) return true;
    return "indeterminate";
  }, [selected, scanned]);

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4">
        <div className="flex items-center justify-between mb-2">
          <h1 className="text-3xl font-black tracking-tight text-foreground">Import Games</h1>
          <Badge variant="secondary" className="bg-primary/10 text-primary border-primary/20 font-bold uppercase tracking-widest text-[10px]">
            {scanned?.length.toString() ?? '0'} Detected
          </Badge>
        </div>
        <p className="text-sm text-muted-foreground max-w-2xl">
          We've found these games installed on your system. Select the ones you'd like to track for save game versioning.
        </p>
      </header>

      <div className="flex-1 overflow-auto p-8 pt-4 scrollbar-hide">
        {isLoading ? (
          <div className="space-y-3">
            {Array.from({ length: 5 }).map((_, i) => (
              <Skeleton key={i.toString()} className="h-16 w-full rounded-xl" />
            ))}
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center h-64 gap-4 bg-sf/40 border border-dashed border-border/40 rounded-2xl">
            <Info className="text-muted-foreground/40" size={32} />
            <p className="text-sm text-muted-foreground font-medium">Could not detect installed games</p>
            <Button variant="outline" size="sm" onClick={() => { window.location.reload(); }}>Retry Search</Button>
          </div>
        ) : scanned?.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-64 gap-4 bg-sf/40 border border-dashed border-border/40 rounded-2xl text-center px-8">
            <CheckCircle2 className="text-primary/60" size={32} />
            <div>
              <p className="text-sm text-foreground font-bold">You're all set!</p>
              <p className="text-xs text-muted-foreground mt-1">All detected games are already being tracked in your library.</p>
            </div>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="flex items-center gap-4 px-6 py-3 bg-sf/40 border border-border/40 rounded-xl mb-4">
              <Checkbox id="select-all" checked={allChecked} onCheckedChange={(s) => { handleToggleAll(s); }} />
              <label htmlFor="select-all" className="text-xs font-bold uppercase tracking-wider text-muted-foreground/70 flex-1 cursor-pointer">
                Select All
              </label>
            </div>

            {scanned?.map((item, idx) => (
              <div 
                key={`${item.title}-${idx.toString()}`} 
                className="flex items-center gap-4 p-4 px-6 rounded-xl bg-sf border border-border/40 hover:border-primary/20 transition-all cursor-pointer group"
                onClick={() => { handleToggleItem(item, !selected.has(item.title)); }}
              >
                <Checkbox 
                  checked={selected.has(item.title)} 
                  onCheckedChange={(s) => { handleToggleItem(item, s); }}
                  onClick={(e) => { e.stopPropagation(); }} 
                />
                <GameIcon title={item.title} size={40} />
                <div className="flex-1 min-w-0">
                  <h3 className="text-sm font-bold tracking-tight text-foreground">{item.title}</h3>
                  <div className="flex items-center gap-2 mt-0.5">
                    <Badge variant="outline" className="text-[9px] h-4 font-bold uppercase tracking-tighter bg-el/50 px-1.5 border-border/60">
                      {item.icon === 'steam' ? 'Steam' : item.icon === 'epic' ? 'Epic' : 'Local'}
                    </Badge>
                    <span className="text-[10px] text-muted-foreground font-medium truncate">
                      {item.dirs.length.toString()} save folders found
                    </span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <footer className="p-8 py-6 border-t border-border/40 bg-background/80 backdrop-blur-md flex items-center justify-between">
        <div className="text-xs font-medium text-muted-foreground">
          {selected.size.toString()} games selected for import
        </div>
        <div className="flex items-center gap-3">
          <Button variant="ghost" onClick={() => { router.history.back(); }}>
            Cancel
          </Button>
          <Button 
            onClick={() => { handleImport(); }} 
            disabled={selected.size === 0 || importMutation.isPending}
            className="font-bold px-8"
          >
            {importMutation.isPending ? "Importing..." : "Import Selected"}
          </Button>
        </div>
      </footer>
    </div>
  );
}

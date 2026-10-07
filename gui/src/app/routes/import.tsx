import { createFileRoute, useRouter } from '@tanstack/react-router';
import { ReactElement, useMemo, useState } from 'react';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { saveGame, setGameIcon } from '@/ipc/commands';
import type { InstallScannerItem } from '@/ipc/bindings/InstallScannerItem';
import { reportError } from '@/shared/errors/report';
import { Button } from '@/shared/ui/button';
import { Checkbox } from '@/shared/ui/checkbox';
import { Badge } from '@/shared/ui/badge';
import { GameIcon } from '@/features/library/GameIcon';
import { Spinner } from '@/shared/ui/spinner';
import { toast } from 'sonner';
import { CheckCircle2, Info, Search, Sparkles } from 'lucide-react';
import { CheckedState } from '@radix-ui/react-checkbox';
import { useScannedGames } from '@/features/add-games/queries';
import { ScrollArea } from '@/shared/ui/scroll-area';

export const Route = createFileRoute('/import')({
  component: ImportGames,
});

function ImportGames(): ReactElement {
  const router = useRouter();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [hasRequestedScan, setHasRequestedScan] = useState(false);

  const { data: scanned, isLoading, error, refetch, isFetching } = useScannedGames(hasRequestedScan);

  const importMutation = useMutation({
    mutationFn: async (items: InstallScannerItem[]) => {
      const results = await Promise.all(
        items.map(async (i) => {
          const saved = await saveGame({ id: null, title: i.title }, i.dirs);
          if (i.icon_path !== null) {
            try {
              await setGameIcon(saved.id, i.icon_path);
            } catch (err) {
              console.warn(`Failed to set icon for "${i.title}":`, err);
            }
          }
          return saved;
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
      reportError('Failed to import games', err);
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

  const handleStartScan = (): void => {
    setSelected(new Set());
    setHasRequestedScan(true);
    void refetch();
  };

  const allChecked: CheckedState = useMemo(() => {
    if (!scanned || scanned.length === 0) return false;
    if (selected.size === 0) return false;
    if (selected.size === scanned.length) return true;
    return "indeterminate";
  }, [selected, scanned]);

  const sortedScanned = useMemo(() => {
    if (!scanned) return scanned;
    return [...scanned].sort((a, b) => {
      const at = a.last_modified_at ?? null;
      const bt = b.last_modified_at ?? null;
      if (at === null && bt === null) return a.title.localeCompare(b.title);
      if (at === null) return 1;
      if (bt === null) return -1;
      return bt - at;
    });
  }, [scanned]);

  const isScanning = isLoading || isFetching;
  const detectedCount = scanned?.length ?? 0;
  const scanStateLabel = !hasRequestedScan ? 'Ready' : isScanning ? 'Scanning' : 'Detected';
  const scanBadgeText = !hasRequestedScan ? scanStateLabel : `${String(detectedCount)} ${scanStateLabel}`;

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4">
        <div className="flex items-center justify-between mb-2">
          <h1 className="text-3xl font-black tracking-tight text-foreground">Import Games</h1>
          <Badge variant="secondary" className="bg-primary/10 text-primary border-primary/20 font-bold uppercase tracking-widest text-[10px]">
            {scanBadgeText}
          </Badge>
        </div>
        {!hasRequestedScan ? (
          <div className="max-w-3xl space-y-4">
            <p className="text-sm text-muted-foreground">
              Scan your installed libraries and known save locations, then choose which detected games to import into your timeline.
            </p>
            <div className="flex flex-wrap items-center gap-3">
              <Button onClick={() => { handleStartScan(); }} className="font-bold px-6 gap-2">
                <Search size={15} />
                Start Scan
              </Button>
              <div className="inline-flex items-center gap-2 rounded-full border border-primary/20 bg-primary/8 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.18em] text-primary/90">
                <Sparkles size={12} />
                Powered by ludusavi-manifest
              </div>
            </div>
          </div>
        ) : isScanning ? (
          <p className="text-sm text-muted-foreground max-w-2xl">
            We’re checking your configured install roots and known save locations. This can take a moment depending on your libraries.
          </p>
        ) : (
          <p className="text-sm text-muted-foreground max-w-2xl">
            We found these games installed on your system. Select the ones you’d like to track for save game versioning.
          </p>
        )}
      </header>

      {/* Custom Scan Roots Section */}
      { /* <div className="mx-8 mb-4 border border-border/40 rounded-2xl bg-sf/30 backdrop-blur-sm overflow-hidden transition-all duration-300">
        <div
          role="button"
          tabIndex={0}
          onClick={() => { setIsRootsExpanded(!isRootsExpanded); }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              setIsRootsExpanded(!isRootsExpanded);
            }
          }}
          className="w-full flex items-center justify-between p-4 px-6 hover:bg-sf/40 transition-colors text-left cursor-pointer"
          aria-expanded={isRootsExpanded}
        >
          <div className="flex items-center gap-3">
            <HardDrive size={18} className="text-primary/70" />
            <div>
              <span className="text-sm font-bold tracking-tight text-foreground">Custom Scan Roots</span>
              <span className="text-xs text-muted-foreground ml-2">({scanRoots?.length ?? 0} folders)</span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={(e) => {
                e.stopPropagation();
                void handlePickDirectory();
              }}
              className="h-8 gap-1.5 px-3 font-semibold text-xs border-primary/20 hover:bg-primary/10 hover:text-primary hover:border-primary/30 cursor-pointer"
            >
              <FolderPlus size={14} />
              Add Root
            </Button>
            {isRootsExpanded ? <ChevronUp size={16} className="text-muted-foreground" /> : <ChevronDown size={16} className="text-muted-foreground" />}
          </div>
        </div>

        {isRootsExpanded && (
          <div className="border-t border-border/40 p-6 pt-4 bg-sf/10 space-y-3">
            <p className="text-xs text-muted-foreground">
              Add folders where your game libraries or DRM-free installations reside. We will scan their subfolders for saves.
            </p>
            {scanRoots === undefined ? (
              <div className="h-10 w-full rounded-xl border border-border/30 bg-sf/60 animate-pulse" />
            ) : scanRoots.length === 0 ? (
              <div className="text-center py-6 text-xs text-muted-foreground/60 border border-dashed border-border/30 rounded-xl">
                No custom scan roots configured.
              </div>
            ) : (
              <ScrollArea className="max-h-48" viewportClassName="pr-3">
              <div className="space-y-2">
                {scanRoots.map((path) => (
                  <div
                    key={path}
                    className="flex items-center justify-between p-2.5 px-4 rounded-xl bg-sf border border-border/30 hover:border-primary/10 transition-all text-xs group"
                  >
                    <span className="font-mono text-muted-foreground select-all truncate max-w-[85%]">
                      {path}
                    </span>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => { removeRootMutation.mutate(path); }}
                      disabled={removeRootMutation.isPending}
                      className="h-7 w-7 text-muted-foreground hover:text-destructive hover:bg-destructive/10 rounded-lg cursor-pointer opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
                    >
                      <Trash2 size={13} />
                    </Button>
                  </div>
                ))}
              </div>
              </ScrollArea>
            )}
          </div>
        )}
      </div>
      */ }

      <ScrollArea className="flex-1 min-h-0" viewportClassName="p-8 pt-4">
      <div>
        {!hasRequestedScan ? (
          <div className="flex flex-col items-center justify-center min-h-[22rem] rounded-[2rem] border border-dashed border-border/40 bg-[radial-gradient(circle_at_top,rgba(98,143,255,0.16),transparent_45%),linear-gradient(180deg,rgba(17,19,32,0.95),rgba(11,12,17,0.98))] px-8 text-center">
            <div className="relative mb-6 flex h-24 w-24 items-center justify-center">
              <div className="absolute inset-0 rounded-full border border-primary/20 bg-primary/8" />
              <div className="absolute inset-2 rounded-full border border-primary/16 animate-[scanPulse_2.6s_ease-in-out_infinite]" />
              <div className="absolute inset-5 rounded-full bg-primary/12 blur-md animate-[scanGlow_2.6s_ease-in-out_infinite]" />
              <Search size={28} className="relative z-10 text-primary" />
            </div>
            <p className="text-lg font-black tracking-tight text-foreground">Ready to scan your libraries</p>
            <p className="mt-2 max-w-xl text-sm leading-6 text-muted-foreground">
              We’ll look through Steam, Epic, custom roots, and known save locations, then show only games that are not already tracked.
            </p>
            <p className="mt-5 text-[11px] font-semibold uppercase tracking-[0.22em] text-primary/80">
              Powered by ludusavi-manifest
            </p>
          </div>
        ) : isScanning ? (
          <div className="flex flex-col items-center justify-center min-h-[22rem] rounded-[2rem] border border-border/40 bg-[radial-gradient(circle_at_top,rgba(98,143,255,0.18),transparent_42%),linear-gradient(180deg,rgba(17,19,32,0.94),rgba(11,12,17,0.98))] px-8 text-center overflow-hidden relative">
            <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent animate-[scanSweep_2.4s_linear_infinite]" />
            <div className="relative mb-6 flex h-28 w-28 items-center justify-center">
              <div className="absolute inset-0 rounded-full border border-primary/20" />
              <div className="absolute inset-3 rounded-full border border-primary/30 animate-[scanPulse_2.2s_ease-out_infinite]" />
              <div className="absolute inset-6 rounded-full border border-primary/20 animate-[scanPulse_2.2s_ease-out_0.35s_infinite]" />
              <div className="absolute inset-0 rounded-full bg-primary/6 blur-2xl animate-[scanGlow_2.2s_ease-in-out_infinite]" />
              <Spinner className="relative z-10 size-8 text-primary" />
            </div>
            <p className="text-lg font-black tracking-tight text-foreground">Scanning for installed games</p>
            <p className="mt-2 max-w-xl text-sm leading-6 text-muted-foreground">
              We’re resolving manifest paths, checking your configured roots, and filtering out already imported games.
            </p>
            <div className="mt-6 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.22em] text-primary/80">
              <span className="h-1.5 w-1.5 rounded-full bg-primary animate-pulse" />
              Powered by ludusavi-manifest
            </div>
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center h-64 gap-4 bg-sf/40 border border-dashed border-border/40 rounded-2xl">
            <Info className="text-muted-foreground/40" size={32} />
            <p className="text-sm text-muted-foreground font-medium">Could not detect installed games</p>
            <Button variant="outline" size="sm" onClick={() => { void refetch(); }}>Retry Scan</Button>
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

            {sortedScanned?.map((item, idx) => (
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
                <GameIcon title={item.title} iconPath={item.icon_path ?? undefined} size={40} />
                <div className="flex-1 min-w-0">
                  <h3 className="text-sm font-bold tracking-tight text-foreground">{item.title}</h3>
                  <div className="flex items-center gap-2 mt-0.5">
                    <Badge variant="outline" className="text-[9px] h-4 font-bold uppercase tracking-tighter bg-el/50 px-1.5 border-border/60">
                      {item.source === 'steam' ? 'Steam' : item.source === 'epic' ? 'Epic' : item.source === 'custom' ? 'Custom' : 'Local'}
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
      </ScrollArea>

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
            disabled={selected.size === 0 || importMutation.isPending || isScanning}
            className="font-bold px-8"
          >
            {importMutation.isPending ? "Importing..." : "Import Selected"}
          </Button>
        </div>
      </footer>
    </div>
  );
}

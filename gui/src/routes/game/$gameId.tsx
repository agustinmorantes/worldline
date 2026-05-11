import { createFileRoute, useRouter } from '@tanstack/react-router';
import { ReactElement, useState } from 'react';
import { motion } from 'framer-motion';
import { useGame } from '@/api/queries';
import { GameIcon } from '@/components/GameIcon';
import { Skeleton } from '@/components/ui/skeleton';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { BranchingTimeline } from '@/components/BranchingTimeline';
import { RestoreModal } from '@/components/RestoreModal';
import { SlotSelector } from '@/components/SlotSelector';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Trash2, Plus, LayoutGrid, ListTree, MapPin, Pencil, Image } from 'lucide-react';
import { Version } from '@/models/version';
import { Game } from '@/models/game';
import { toast } from 'sonner';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator
} from '@/components/ui/dropdown-menu';
import { deleteGame, saveGame, setGameIcon } from '@/api/games';
import { queryClient } from '@/lib/query-client';
import { confirm, open } from '@tauri-apps/plugin-dialog';
import { ScrollArea } from '@/components/ui/scroll-area';

export const Route = createFileRoute('/game/$gameId')({
  component: GameDetail,
});

function GameDetail(): ReactElement {
  const router = useRouter();
  const { gameId } = Route.useParams();
  const id = parseInt(gameId);
  const { data: fullGame, isLoading, error, refetch } = useGame(id);

  const [restoreTarget, setRestoreTarget] = useState<Version | null>(null);
  const [editTitleOpen, setEditTitleOpen] = useState(false);
  const [editTitleValue, setEditTitleValue] = useState('');

  if (isLoading) return <GameDetailSkeleton />;

  if (error || !fullGame) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4">
        <div className="text-destructive font-bold">Failed to load game</div>
        <Button onClick={() => { void refetch(); }}>Retry</Button>
      </div>
    );
  }

  const game = fullGame;
  const { versions, tracked_dirs } = fullGame;

  async function handleDelete(): Promise<void> {
    const confirmed = await confirm(
      `Are you sure you want to delete ${game.title}? This will not remove physical save files.`,
      { title: 'Delete Game', kind: 'warning' }
    );
    if (!confirmed) return;
    try {
      await deleteGame(game.id);
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void router.navigate({ to: '/' });
      toast.success('Game deleted');
    } catch (err) {
      console.error(err);
      toast.error('Failed to delete game');
    }
  }

  async function handleConfirmRestore(): Promise<void> {
    if (restoreTarget === null) return;
    const v = restoreTarget;
    setRestoreTarget(null);
    try {
      await v.restore();
      void queryClient.invalidateQueries({ queryKey: ['game', id] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      toast.success(`Restored v${v.version_number.toString()}`);
    } catch (err) {
      console.error(err);
      toast.error('Failed to restore version');
    }
  }

  async function handleTag(v: Version, tag: string): Promise<void> {
    if (tag === (v.tag ?? '')) return;
    try {
      v.tag = tag;
      await v.save();
      void queryClient.invalidateQueries({ queryKey: ['game', id] });
      toast.success('Version updated');
    } catch (err) {
      console.error(err);
      toast.error('Failed to update version tag');
    }
  }

  async function handleChangeIcon(): Promise<void> {
    const selected = await open({
      multiple: false,
      title: 'Select Game Icon',
      filters: [{ name: 'Images & Executables', extensions: ['png', 'jpg', 'jpeg', 'ico', 'bmp', 'webp', 'exe'] }],
    });
    if (selected === null) return;
    try {
      await setGameIcon(game.id, selected);
      void queryClient.invalidateQueries({ queryKey: ['game', id] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      toast.success('Icon updated');
    } catch (err) {
      console.error(err);
      toast.error('Failed to set icon');
    }
  }

  function handleOpenEditTitle(): void {
    setEditTitleValue(game.title);
    setEditTitleOpen(true);
  }

  async function handleSaveTitle(): Promise<void> {
    const newTitle = editTitleValue.trim();
    if (newTitle === '' || newTitle === game.title) {
      setEditTitleOpen(false);
      return;
    }
    try {
      const updated = new Game(newTitle);
      updated.id = game.id;
      updated.active_version_id = game.active_version_id;
      updated.tracked_dirs = tracked_dirs;
      await saveGame(updated);
      void queryClient.invalidateQueries({ queryKey: ['game', id] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      setEditTitleOpen(false);
      toast.success('Title updated');
    } catch (err) {
      console.error(err);
      toast.error('Failed to update title');
    }
  }

  async function handleAddDirectory(): Promise<void> {
    const selected = await open({ directory: true, multiple: false, title: 'Select Save Directory' });
    if (selected === null) return;
    try {
      const updated = new Game(game.title);
      updated.id = game.id;
      updated.active_version_id = game.active_version_id;
      updated.tracked_dirs = [...tracked_dirs, { path: selected }];
      await saveGame(updated);
      void queryClient.invalidateQueries({ queryKey: ['game', id] });
      toast.success('Directory added');
    } catch (err) {
      console.error(err);
      toast.error('Failed to add directory');
    }
  }

  return (
    <div className="flex flex-col h-full bg-background overflow-hidden">
      <header className="p-8 pb-4 flex items-center gap-6">
        <motion.div
          key={game.id}
          initial={{ scale: 0.9, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ duration: 0.38, ease: [0.16, 1, 0.3, 1] }}
        >
          <GameIcon title={game.title} size={64} iconPath={game.icon_path} />
        </motion.div>
        <div className="flex-1 flex flex-col gap-1">
          <div className="flex items-center gap-3">
            <h1 className="text-3xl font-black tracking-tight text-foreground">{game.title}</h1>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm" className="h-7 px-3 text-[10px] font-bold uppercase tracking-wider">
                  Options
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onClick={() => { void handleChangeIcon(); }}>
                  <Image size={14} className="mr-2" /> Change Icon
                </DropdownMenuItem>
                <DropdownMenuItem onClick={handleOpenEditTitle}>
                  <Pencil size={14} className="mr-2" /> Edit Title
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem variant="destructive" onClick={() => { void handleDelete(); }}>
                  <Trash2 size={14} className="mr-2" /> Delete Game
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          <div className="flex items-center gap-3">
            <StatChip label={`${versions.length.toString()} total saves`} />
            <StatChip label="1 slot" />
          </div>
        </div>
      </header>

      <Tabs defaultValue="timeline" className="flex-1 flex flex-col overflow-hidden">
        <div className="px-8 border-b border-border/40">
          <TabsList className="bg-transparent h-12 p-0 gap-8">
            <TabsTrigger
              value="timeline"
              className="bg-transparent data-[state=active]:bg-transparent data-[state=active]:shadow-none border-b-2 border-transparent data-[state=active]:border-primary rounded-none h-12 px-0 text-sm font-semibold tracking-tight"
            >
              <ListTree size={16} className="mr-2" /> Save Timeline
            </TabsTrigger>
            <TabsTrigger
              value="dirs"
              className="bg-transparent data-[state=active]:bg-transparent data-[state=active]:shadow-none border-b-2 border-transparent data-[state=active]:border-primary rounded-none h-12 px-0 text-sm font-semibold tracking-tight"
            >
              <MapPin size={16} className="mr-2" /> Tracked Dirs
            </TabsTrigger>
          </TabsList>
        </div>

        <ScrollArea className="flex-1 min-h-0">
        <div>
          <TabsContent value="timeline" className="m-0 p-8 pt-6">
            <div className="flex items-center justify-between mb-8">
              <SlotSelector />
              <div className="flex items-center gap-1 p-1 bg-el rounded-lg">
                <Button variant="ghost" size="icon" className="h-7 w-7 rounded-md bg-sf shadow-sm text-primary">
                  <ListTree size={14} />
                </Button>
                <Button variant="ghost" size="icon" className="h-7 w-7 rounded-md text-muted-foreground/40">
                  <LayoutGrid size={14} />
                </Button>
              </div>
            </div>

            <BranchingTimeline
              versions={versions}
              activeVersionId={game.active_version_id ?? undefined}
              onRestore={(v) => { setRestoreTarget(v); }}
              onTag={(v, tag) => { void handleTag(v, tag); }}
            />
          </TabsContent>

          <TabsContent value="dirs" className="m-0 p-8 pt-6">
            <div className="max-w-3xl space-y-4">
              <div className="flex items-center justify-between mb-6">
                <div>
                  <h2 className="text-lg font-bold tracking-tight">Tracked Directories</h2>
                  <p className="text-sm text-muted-foreground">The folders currently being monitored for save game changes.</p>
                </div>
                <Button size="sm" className="gap-2" onClick={() => { void handleAddDirectory(); }}>
                  <Plus size={16} /> Add Directory
                </Button>
              </div>

              <div className="grid gap-3">
                {tracked_dirs.map((dir, idx) => (
                  <div key={dir.id?.toString() ?? idx.toString()} className="flex items-center gap-4 p-4 rounded-xl bg-sf border border-border/40 group hover:border-primary/20 transition-all">
                    <div className="w-10 h-10 rounded-lg bg-el flex items-center justify-center text-muted-foreground shrink-0">
                      <MapPin size={20} />
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-mono truncate text-tx2">{dir.path}</p>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </TabsContent>
        </div>
        </ScrollArea>
      </Tabs>

      <RestoreModal
        version={restoreTarget}
        onConfirm={() => { void handleConfirmRestore(); }}
        onCancel={() => { setRestoreTarget(null); }}
      />

      <Dialog open={editTitleOpen} onOpenChange={setEditTitleOpen}>
        <DialogContent className="sm:max-w-[400px] bg-sf border-border/40">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">Edit Game Title</DialogTitle>
          </DialogHeader>
          <Input
            value={editTitleValue}
            onChange={(e) => { setEditTitleValue(e.target.value); }}
            className="bg-el border-border/40"
            onKeyDown={(e) => { if (e.key === 'Enter') void handleSaveTitle(); }}
            autoFocus
          />
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="ghost" onClick={() => { setEditTitleOpen(false); }}>Cancel</Button>
            <Button onClick={() => { void handleSaveTitle(); }} className="font-bold">Save</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function StatChip({ label }: { label: string }): ReactElement {
  return (
    <div className="px-2.5 py-1 rounded-md bg-el border border-border/40 text-[10px] font-bold uppercase tracking-wider text-muted-foreground/80 flex items-center gap-1.5">
      <div className="w-1 h-1 rounded-full bg-primary/40" />
      {label}
    </div>
  );
}

function GameDetailSkeleton(): ReactElement {
  return (
    <div className="flex flex-col h-full bg-background animate-pulse">
      <header className="p-8 pb-4 flex items-center gap-6">
        <Skeleton className="w-16 h-16 rounded-lg" />
        <div className="flex-1 space-y-2">
          <Skeleton className="h-8 w-64" />
          <div className="flex gap-2">
            <Skeleton className="h-5 w-24" />
            <Skeleton className="h-5 w-24" />
          </div>
        </div>
      </header>
      <div className="px-8 border-b border-border/40">
        <div className="h-12 flex gap-8">
          <Skeleton className="h-full w-32" />
          <Skeleton className="h-full w-32" />
        </div>
      </div>
      <div className="p-8 space-y-6">
        <div className="flex justify-between">
          <Skeleton className="h-8 w-48" />
          <Skeleton className="h-8 w-24" />
        </div>
        <div className="space-y-4">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={`skeleton-${i.toString()}`} className="h-16 w-full rounded-xl" />
          ))}
        </div>
      </div>
    </div>
  );
}

// The header's "…" menu. P4 replaces it with the game settings sheet; until then it keeps the
// old view's actions reachable.
import { type ReactElement, useState } from 'react';
import { useRouter } from '@tanstack/react-router';
import * as Menu from '@radix-ui/react-dropdown-menu';
import { toast } from 'sonner';
import type { FullGame } from '@/ipc/bindings/FullGame';
import { confirm, exportGameHistory, pickFile, pickFolder, pickSavePath, saveGame, setGameIcon, untrackGame } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { Button } from '@/shared/ui/button';
import { Input } from '@/shared/ui/input';
import { cn } from '@/shared/ui/utils';
import { chip, popover, row } from './PlaythroughPicker';

const refresh = (id: number): void => {
  void queryClient.invalidateQueries({ queryKey: ['game', id] });
  void queryClient.invalidateQueries({ queryKey: ['games'] });
};

export function GameMenu({ game }: { game: FullGame }): ReactElement {
  const t = useT();
  const router = useRouter();
  const [renaming, setRenaming] = useState<string | null>(null);
  const dirs = game.tracked_dirs.map((d) => d.path);

  async function changeIcon(): Promise<void> {
    const selected = await pickFile(t.game.pickIcon, [{ name: t.game.iconFiles, extensions: ['png', 'jpg', 'jpeg', 'ico', 'bmp', 'webp', 'exe'] }]);
    if (selected === null) return;
    try {
      await setGameIcon(game.id, selected);
      refresh(game.id);
    } catch (err) {
      reportError(t.game.failed.icon, err);
    }
  }

  async function rename(): Promise<void> {
    const title = renaming?.trim() ?? '';
    setRenaming(null);
    if (title === '' || title === game.title) return;
    try {
      await saveGame({ id: game.id, title }, dirs);
      refresh(game.id);
    } catch (err) {
      reportError(t.game.failed.rename, err);
    }
  }

  async function addFolder(): Promise<void> {
    const selected = await pickFolder(t.game.pickSaveFolder);
    if (selected === null) return;
    try {
      await saveGame({ id: game.id, title: game.title }, [...dirs, selected]);
      refresh(game.id);
    } catch (err) {
      reportError(t.game.failed.folder, err);
    }
  }

  async function exportHistory(): Promise<void> {
    try {
      const destination = await pickSavePath(t.game.exportTo, t.game.exportName(game.title));
      if (destination === null) return;
      const report = await exportGameHistory(game.id, destination);
      toast.success(t.game.exported(report.version_count, report.title));
    } catch (err) {
      reportError(t.game.failed.export, err);
    }
  }

  async function remove(): Promise<void> {
    if (!(await confirm(t.game.removeConfirmTitle, t.game.removeConfirm(game.title)))) return;
    try {
      await untrackGame(game.id);
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void router.navigate({ to: '/' });
    } catch (err) {
      reportError(t.game.failed.remove, err);
    }
  }

  return (
    <>
      <Menu.Root>
        <Menu.Trigger className={cn(chip, 'w-[30px] justify-center px-0')} aria-label={t.game.moreOptions}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content align="start" sideOffset={6} className={`${popover} w-[240px]`}>
            <Menu.Item className={row} onSelect={() => { void changeIcon(); }}>{t.game.changeIcon}</Menu.Item>
            <Menu.Item className={row} onSelect={() => { setRenaming(game.title); }}>{t.game.renameGame}</Menu.Item>
            <Menu.Item className={row} onSelect={() => { void addFolder(); }}>
              <span className="flex-1">{t.game.addSaveFolder}</span>
              <span className="font-mono text-xs text-tx3">{dirs.length}</span>
            </Menu.Item>
            <Menu.Item className={row} onSelect={() => { void exportHistory(); }}>{t.game.exportHistory}</Menu.Item>
            <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />
            <Menu.Item className={`${row} text-err`} onSelect={() => { void remove(); }}>{t.game.removeFromPc}</Menu.Item>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
      <Dialog open={renaming !== null} onOpenChange={(open) => { if (!open) setRenaming(null); }}>
        <DialogContent className="border-border/40 bg-sf sm:max-w-[400px]">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.game.renameGame}</DialogTitle>
          </DialogHeader>
          <Input
            value={renaming ?? ''}
            onChange={(e) => { setRenaming(e.target.value); }}
            onKeyDown={(e) => { if (e.key === 'Enter') void rename(); }}
            className="border-border/40 bg-el"
            autoFocus
          />
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="ghost" onClick={() => { setRenaming(null); }}>{t.game.cancel}</Button>
            <Button onClick={() => { void rename(); }} className="font-bold">{t.game.rename}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

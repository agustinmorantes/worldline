// The game's menu: everything about the game itself, from its name and color to removing it.
// The header's "…" button and a right-click on the game's sidebar row open the same items.
import { Fragment, type ReactElement, type ReactNode, useState } from 'react';
import { useRouter } from '@tanstack/react-router';
import { ContextMenu, DropdownMenu } from 'radix-ui';
import { Check, ChevronRight, Plus, Trash2, X } from 'lucide-react';
import { toast } from 'sonner';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { RemoteGameSummary } from '@/ipc/bindings/RemoteGameSummary';
import type { SyncReport } from '@/ipc/bindings/SyncReport';
import { deleteGame, exportGameHistory, pickFile, pickFolder, pickSavePath, recoverGame, saveGame, setGameAccent, setGameIcon, untrackGame } from '@/ipc/commands';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { useT } from '@/shared/i18n';
import { setPref, usePrefs } from '@/shared/prefs/prefs';
import { CORNERS, type GameCorner } from '@/shared/prefs/schema';
import { useShellNav } from '@/features/library/shell-nav';
import { accentVars } from '@/shared/tokens/accent';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { btn, btnQuiet, chip, field, popover, row } from '@/shared/ui/classes';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';

const HUES = Array.from({ length: 12 }, (_, i) => i * 30);

const refresh = (id: number): void => {
  void queryClient.invalidateQueries({ queryKey: ['game', id] });
  void queryClient.invalidateQueries({ queryKey: ['games'] });
};

type Open = 'rename' | 'folders' | 'delete' | null;

/** What both menus render items with: the context menu's parts take the same props as the dropdown's. */
type MenuParts = Pick<typeof DropdownMenu, 'Item' | 'Sub' | 'SubTrigger' | 'SubContent' | 'Portal' | 'RadioGroup' | 'RadioItem' | 'ItemIndicator' | 'Separator'>;

interface GameMenuProps {
  game: FullGame;
  /** Removing from this PC needs the history safe in the cloud. */
  inCloud: boolean;
}

/** The header's "…" button. */
export function GameMenu({ game, inCloud }: GameMenuProps): ReactElement {
  const t = useT();
  const { items, dialogs } = useGameMenu(game, inCloud);
  return (
    <>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger className={cn(chip, 'w-[30px] justify-center px-0')} aria-label={t.game.moreOptions}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="start" sideOffset={6} className={`${popover} w-[240px]`}>{items(DropdownMenu)}</DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {dialogs}
    </>
  );
}

/** Right-clicking the `ContextMenu.Trigger` among `children` opens the game's menu. */
export function GameContextMenu({ game, inCloud, children }: GameMenuProps & { children: ReactNode }): ReactElement {
  const { items, dialogs } = useGameMenu(game, inCloud);
  return (
    <>
      <ContextMenu.Root>
        {children}
        <ContextMenu.Portal>
          <ContextMenu.Content className={`${popover} w-[240px]`}>{items(ContextMenu)}</ContextMenu.Content>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      {dialogs}
    </>
  );
}

/** The menu's items, for either kind of menu, and the dialogs they open. */
function useGameMenu(game: FullGame, inCloud: boolean): { items: (Menu: MenuParts) => ReactElement; dialogs: ReactElement } {
  const t = useT();
  const router = useRouter();
  const { current } = useShellNav();
  const [open, setOpen] = useState<Open>(null);
  const [title, setTitle] = useState('');
  const dirs = game.tracked_dirs.map((d) => d.path);

  async function run(action: () => Promise<unknown>, failed: string): Promise<boolean> {
    try {
      await action();
      refresh(game.id);
      return true;
    } catch (err) {
      reportError(failed, err);
      return false;
    }
  }

  async function changeIcon(): Promise<void> {
    const selected = await pickFile(t.game.pickIcon, [{ name: t.game.iconFiles, extensions: ['png', 'jpg', 'jpeg', 'ico', 'bmp', 'webp', 'exe'] }]);
    if (selected !== null) await run(() => setGameIcon(game.id, selected), t.game.failed.icon);
  }

  async function rename(): Promise<void> {
    const next = title.trim();
    setOpen(null);
    if (next !== '' && next !== game.title) await run(() => saveGame({ id: game.id, title: next }, dirs), t.game.failed.rename);
  }

  async function setFolders(next: string[]): Promise<void> {
    await run(() => saveGame({ id: game.id, title: game.title }, next), t.game.failed.folder);
  }

  async function addFolder(): Promise<void> {
    const selected = await pickFolder(t.game.pickSaveFolder);
    if (selected !== null && !dirs.includes(selected)) await setFolders([...dirs, selected]);
  }

  async function exportHistory(): Promise<void> {
    try {
      const destination = await pickSavePath(t.game.exportTo, t.game.exportName(game.title), [{ name: t.game.bundleFiles, extensions: ['zip'] }]);
      if (destination === null) return;
      const report = await exportGameHistory(game.id, destination);
      toast.success(t.game.exported(report.version_count, report.title));
    } catch (err) {
      reportError(t.game.failed.export, err);
    }
  }

  /** Runs `action`, leaves the game, and updates the cloud list with `cloud` right away: the
   *  sync that would tell it can take a while. False when it failed. */
  async function leave(action: () => Promise<void>, cloud: (games: RemoteGameSummary[]) => RemoteGameSummary[], failed: string, done?: string): Promise<boolean> {
    try {
      await action();
      setOpen(null);
      // From the sidebar it may be another game than the one on screen; then the screen stays.
      if (current?.view === 'game' && current.gameId === game.id) void router.navigate({ to: '/' });
      queryClient.setQueryData<SyncReport>(['sync-report'], (old) => old && { ...old, remote_games: cloud(old.remote_games) });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
      void queryClient.invalidateQueries({ queryKey: ['sync-report'] });
      if (done !== undefined) toast.success(done);
      return true;
    } catch (err) {
      reportError(failed, err);
      return false;
    }
  }

  /** Deletes the game everywhere, with an undo in the toast: it waits in Recently deleted. */
  async function deleteEverywhere(): Promise<void> {
    const left = await leave(() => deleteGame(game.id), (games) => games.filter((g) => g.uuid !== game.uuid), t.game.failed.delete);
    void queryClient.invalidateQueries({ queryKey: ['recently-deleted'] });
    // Longer than the usual toast, so there is time to reach Undo.
    if (left) toast.success(t.game.deleted(game.title), { duration: 10_000, action:{ label: t.game.undo, onClick: () => { void undoDelete(); } } });
  }

  async function undoDelete(): Promise<void> {
    try {
      await recoverGame(game.uuid);
    } catch (err) {
      // Still deleted, so still in Recently deleted; trying again here is as good.
      reportError(t.game.failed.undo, err, t.game.undoHint, { label: t.errors.retry, onClick: () => { void undoDelete(); } });
    }
    void queryClient.invalidateQueries({ queryKey: ['games'] });
    void queryClient.invalidateQueries({ queryKey: ['sync-report'] });
    void queryClient.invalidateQueries({ queryKey: ['recently-deleted'] });
  }

  const userHue = game.accent_hue_source === 'user' ? game.accent_hue : null;
  const overlay = usePrefs().overlay;
  const popupCorner = overlay.perGame[game.uuid] ?? 'default';
  // By `uuid`, on this device only: screens differ per device.
  const setPopupCorner = (value: string): void => {
    const { [game.uuid]: _, ...others } = overlay.perGame;
    const perGame = value === 'default' ? others : { ...others, [game.uuid]: value as GameCorner };
    setPref('overlay', { ...overlay, perGame });
  };

  const items = (Menu: MenuParts): ReactElement => (
    <>
      <Menu.Item className={row} onSelect={() => { setTitle(game.title); setOpen('rename'); }}>{t.game.renameGame}</Menu.Item>
      <Menu.Item className={row} onSelect={() => { void changeIcon(); }}>{t.game.changeIcon}</Menu.Item>
      <Menu.Sub>
        <Menu.SubTrigger className={row}>
          <span className="flex-1">{t.game.changeColor}</span>
          <span aria-hidden="true" className="size-3.5 rounded-full" style={{ background: accentVars(game.accent_hue)['--acc'] }} />
          <ChevronRight size={14} aria-hidden="true" className="text-tx3" />
        </Menu.SubTrigger>
        <Menu.Portal>
          <Menu.SubContent sideOffset={8} className={`${popover} w-[208px]`}>
            <div className="grid grid-cols-6 gap-1.5 p-1.5">
              {HUES.map((hue) => (
                <Menu.Item
                  key={hue}
                  aria-label={t.game.colorHue(hue)}
                  className="flex size-7 items-center justify-center rounded-full outline-none transition-transform duration-150 ease-spring data-[highlighted]:scale-110 data-[highlighted]:shadow-[0_0_0_2px_var(--tx2)]"
                  style={{ background: accentVars(hue)['--acc'], color: accentVars(hue)['--acc-ink'] }}
                  onSelect={() => { void run(() => setGameAccent(game.id, hue), t.game.failed.color); }}
                >
                  {userHue === hue && <Check size={14} strokeWidth={3} aria-hidden="true" />}
                </Menu.Item>
              ))}
            </div>
            <Menu.Item className={row} onSelect={() => { void run(() => setGameAccent(game.id, null), t.game.failed.color); }}>
              <span className="flex-1">{t.game.colorAuto}</span>
              {userHue === null && <Check size={14} strokeWidth={3} aria-hidden="true" />}
            </Menu.Item>
          </Menu.SubContent>
        </Menu.Portal>
      </Menu.Sub>
      <Menu.Item className={row} onSelect={() => { setOpen('folders'); }}>
        <span className="flex-1">{t.game.saveFolders}</span>
        <span className="font-mono text-xs text-tx3">{dirs.length}</span>
      </Menu.Item>
      <Menu.Sub>
        <Menu.SubTrigger className={row}>
          <span className="flex-1">{t.overlay.section}</span>
          <ChevronRight size={14} aria-hidden="true" className="text-tx3" />
        </Menu.SubTrigger>
        <Menu.Portal>
          <Menu.SubContent sideOffset={8} className={`${popover} w-[262px]`}>
            <Menu.RadioGroup value={popupCorner} onValueChange={setPopupCorner}>
              {(['default', ...CORNERS, 'off'] as const).map((value) => (
                <Fragment key={value}>
                  {(value === 'br' || value === 'off') && <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />}
                  <Menu.RadioItem value={value} className={row}>
                    <span className="flex w-4 text-acc">
                      <Menu.ItemIndicator><Check size={16} strokeWidth={2.6} aria-hidden="true" /></Menu.ItemIndicator>
                    </span>
                    {value === 'default' ? (
                      <PopupChoice label={t.overlay.useDefault} hint={t.overlay.corners[overlay.corner]} />
                    ) : value === 'off' ? (
                      <PopupChoice label={t.overlay.off} hint={t.overlay.offHint} />
                    ) : (
                      <span className="flex-1">{t.overlay.corners[value]}</span>
                    )}
                  </Menu.RadioItem>
                </Fragment>
              ))}
            </Menu.RadioGroup>
          </Menu.SubContent>
        </Menu.Portal>
      </Menu.Sub>
      <Menu.Item className={row} onSelect={() => { void exportHistory(); }}>{t.game.exportHistory}</Menu.Item>
      <Menu.Separator className="mx-1 my-1.5 h-px bg-[oklch(100%_0_0/0.07)]" />
      <Menu.Item
        className={cn(row, 'flex-col items-start gap-0.5')}
        disabled={!inCloud}
        onSelect={() => {
          void leave(
            () => untrackGame(game.id),
            (games) => games.map((g) => (g.uuid === game.uuid ? { ...g, adopted: false } : g)),
            t.game.failed.remove,
            t.game.removed(game.title),
          );
        }}
      >
        {t.game.removeFromPc}
        {!inCloud && <span className="text-xs font-medium text-tx3">{t.game.removeNeedsCloud}</span>}
      </Menu.Item>
      <Menu.Item className={cn(row, 'text-err')} onSelect={() => { setOpen('delete'); }}>{t.game.deleteEverywhere}</Menu.Item>
    </>
  );

  const dialogs = (
    <>
      <Dialog open={open === 'rename'} onOpenChange={(o) => { if (!o) setOpen(null); }}>
        <DialogContent className="bg-sf sm:max-w-[400px]">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.game.renameGame}</DialogTitle>
          </DialogHeader>
          <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); void rename(); }}>
            <input className={field} value={title} onChange={(e) => { setTitle(e.target.value); }} autoFocus />
            <DialogFooter className="gap-2">
              <button type="button" className={btnQuiet} onClick={() => { setOpen(null); }}>{t.game.cancel}</button>
              <button type="submit" className={btn}>{t.game.rename}</button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={open === 'folders'} onOpenChange={(o) => { if (!o) setOpen(null); }}>
        <DialogContent className="bg-sf sm:max-w-[520px]">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.game.saveFolders}</DialogTitle>
            <DialogDescription className="text-tx3">{t.game.saveFoldersHint}</DialogDescription>
          </DialogHeader>
          <ul className="flex min-w-0 flex-col gap-2">
            {dirs.length === 0 && <li className="text-sm text-tx3">{t.game.noFolders}</li>}
            {dirs.map((path) => (
              <li key={path} className="flex items-center gap-2 rounded-xl bg-el py-2 pl-3 pr-1.5">
                <Tooltip mono content={path}>
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">{path}</span>
                </Tooltip>
                <button
                  type="button"
                  aria-label={t.game.stopWatching(path)}
                  className="flex size-7 shrink-0 items-center justify-center rounded-full text-tx3 transition-colors hover:bg-ln hover:text-tx"
                  onClick={() => { void setFolders(dirs.filter((d) => d !== path)); }}
                >
                  <X size={14} strokeWidth={2.6} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
          <DialogFooter className="gap-2">
            <button type="button" className={btnQuiet} onClick={() => { void addFolder(); }}>
              <Plus size={15} strokeWidth={2.4} aria-hidden="true" />
              {t.game.addFolder}
            </button>
            <button type="button" className={btn} onClick={() => { setOpen(null); }}>{t.game.done}</button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={open === 'delete'} onOpenChange={(o) => { if (!o) setOpen(null); }}>
        <DialogContent className="bg-sf sm:max-w-[420px]">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.game.deleteTitle(game.title)}</DialogTitle>
            <DialogDescription className="text-tx2">{t.game.deleteBody}</DialogDescription>
          </DialogHeader>
          <DialogFooter className="items-center gap-2">
            <button type="button" className={btnQuiet} onClick={() => { setOpen(null); }}>{t.game.cancel}</button>
            <button type="button" className={cn(btn, 'bg-err text-bg')} onClick={() => { void deleteEverywhere(); }}>
              <Trash2 size={15} strokeWidth={2.4} aria-hidden="true" />
              {t.game.delete}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );

  return { items, dialogs };
}

function PopupChoice({ label, hint }: { label: string; hint: string }): ReactElement {
  return (
    <span className="flex flex-1 flex-col gap-0.5">
      <span>{label}</span>
      <span className="text-xs font-medium text-tx3">{hint}</span>
    </span>
  );
}

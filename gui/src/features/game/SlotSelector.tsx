import { ReactElement, useMemo, useState } from 'react';
import { AlertTriangle, Pencil, Plus, Trash2 } from 'lucide-react';
import type { Slot } from '@/ipc/bindings/Slot';
import { Button } from '@/shared/ui/button';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { Input } from '@/shared/ui/input';

interface SlotSelectorProps {
  slots: Slot[];
  activeSlotId?: number;
  disabled?: boolean;
  onSwitch: (slot: Slot) => Promise<void>;
  onCreate: (name: string) => Promise<void>;
  onRename: (slot: Slot, name: string) => Promise<void>;
  /** `replacementSlotId` is only set when deleting the active slot: the backend
   * refuses to leave a game without one, so the UI has to name the slot it will
   * switch to. */
  onDelete: (slot: Slot, replacementSlotId?: number) => Promise<void>;
}

export function SlotSelector({
  slots,
  activeSlotId,
  disabled = false,
  onSwitch,
  onCreate,
  onRename,
  onDelete,
}: SlotSelectorProps): ReactElement {
  const [createOpen, setCreateOpen] = useState(false);
  const [slotName, setSlotName] = useState('');
  const [busySlotId, setBusySlotId] = useState<number | null>(null);
  const [creating, setCreating] = useState(false);
  const [pendingSwitch, setPendingSwitch] = useState<Slot | null>(null);
  const [renameTarget, setRenameTarget] = useState<Slot | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<Slot | null>(null);

  const sortedSlots = useMemo(
    () => [...slots].sort((left, right) => left.id - right.id),
    [slots],
  );

  const busy = disabled || creating || busySlotId !== null;

  // Deleting the active slot has to move the user somewhere. The oldest other
  // slot is the least surprising landing spot, and naming it up front is what
  // lets the confirmation be honest about what is about to happen.
  const deleteReplacement =
    deleteTarget !== null && deleteTarget.id === activeSlotId
      ? sortedSlots.find((slot) => slot.id !== deleteTarget.id)
      : undefined;

  async function handleSwitch(slot: Slot): Promise<void> {
    if (busy || slot.id === activeSlotId) return;

    try {
      setBusySlotId(slot.id);
      await onSwitch(slot);
      setPendingSwitch(null);
    } catch {
      // The caller reports the failure; the dialog stays open for a retry.
    } finally {
      setBusySlotId(null);
    }
  }

  async function handleCreate(): Promise<void> {
    const trimmedName = slotName.trim();
    if (trimmedName === '' || creating || disabled) return;

    try {
      setCreating(true);
      await onCreate(trimmedName);
      setSlotName('');
      setCreateOpen(false);
    } catch {
      // The caller reports the failure; the dialog stays open for a retry.
    } finally {
      setCreating(false);
    }
  }

  async function handleRename(): Promise<void> {
    const trimmedName = renameValue.trim();
    if (renameTarget === null || trimmedName === '' || busy) return;

    try {
      setBusySlotId(renameTarget.id);
      await onRename(renameTarget, trimmedName);
      setRenameTarget(null);
    } catch {
      // The caller reports the failure; the dialog stays open for a retry.
    } finally {
      setBusySlotId(null);
    }
  }

  async function handleDelete(): Promise<void> {
    if (deleteTarget === null || busy) return;
    // The last slot is never deletable, and its trigger is disabled.
    if (deleteTarget.id === activeSlotId && deleteReplacement === undefined) return;

    try {
      setBusySlotId(deleteTarget.id);
      await onDelete(deleteTarget, deleteReplacement?.id);
      setDeleteTarget(null);
    } catch {
      // The caller reports the failure; the dialog stays open for a retry.
    } finally {
      setBusySlotId(null);
    }
  }

  return (
    <>
      <div className="flex items-center gap-2">
        <div className="flex items-center gap-2 overflow-x-auto pb-1">
          {sortedSlots.map((slot) => {
            const isActive = slot.id === activeSlotId;
            const isBusy = busySlotId === slot.id;

            return (
              <div
                key={slot.id}
                className="flex shrink-0 items-center gap-0.5 rounded-md border border-border/40 bg-el/40 p-0.5"
              >
                <Button
                  variant={isActive ? 'default' : 'secondary'}
                  size="sm"
                  disabled={busy}
                  className="h-7 shrink-0 rounded-md px-3 text-xs font-semibold"
                  onClick={() => {
                    if (isActive) return;
                    setPendingSwitch(slot);
                  }}
                >
                  {isBusy ? 'Switching...' : slot.name}
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Rename ${slot.name}`}
                  disabled={busy}
                  className="h-7 w-7 rounded-md text-muted-foreground hover:text-foreground"
                  onClick={() => {
                    setRenameValue(slot.name);
                    setRenameTarget(slot);
                  }}
                >
                  <Pencil size={13} />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Delete ${slot.name}`}
                  disabled={busy || sortedSlots.length <= 1}
                  className="h-7 w-7 rounded-md text-muted-foreground hover:text-destructive"
                  onClick={() => { setDeleteTarget(slot); }}
                >
                  <Trash2 size={13} />
                </Button>
              </div>
            );
          })}
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="h-8 shrink-0 rounded-md px-3 text-xs font-semibold"
          disabled={busy}
          onClick={() => { setCreateOpen(true); }}
        >
          <Plus size={14} className="mr-1.5" />
          New Slot
        </Button>
      </div>

      <Dialog
        open={pendingSwitch !== null}
        onOpenChange={(open) => { if (!open) setPendingSwitch(null); }}
      >
        <DialogContent className="sm:max-w-[420px] bg-sf border-border/40">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">
              Switch to {pendingSwitch?.name}?
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <p className="text-xs text-muted-foreground leading-relaxed">
              The current slot is snapshotted first, then your save files are overwritten with{' '}
              <span className="text-primary font-medium">{pendingSwitch?.name}</span>. Nothing is
              lost — each slot keeps its own timeline.
            </p>
            <div className="flex gap-3 p-3 rounded-lg bg-warn/10 border border-warn/20">
              <AlertTriangle size={16} className="text-warn shrink-0 mt-0.5" />
              <p className="text-xs text-muted-foreground leading-relaxed">
                <span className="text-warn font-bold">Important:</span> Please close the game before
                switching to prevent file corruption or synchronization issues.
              </p>
            </div>
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="ghost" onClick={() => { setPendingSwitch(null); }}>Cancel</Button>
            <Button
              disabled={busy}
              className="font-bold"
              onClick={() => { if (pendingSwitch !== null) void handleSwitch(pendingSwitch); }}
            >
              Switch Slot
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={renameTarget !== null}
        onOpenChange={(open) => { if (!open) setRenameTarget(null); }}
      >
        <DialogContent className="sm:max-w-[400px] bg-sf border-border/40">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">Rename Slot</DialogTitle>
          </DialogHeader>
          <Input
            value={renameValue}
            onChange={(event) => { setRenameValue(event.target.value); }}
            placeholder="Slot name"
            className="bg-el border-border/40"
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                void handleRename();
              }
            }}
            autoFocus
          />
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="ghost" onClick={() => { setRenameTarget(null); }}>Cancel</Button>
            <Button
              disabled={renameValue.trim() === '' || busy}
              onClick={() => { void handleRename(); }}
            >
              Rename
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(open) => { if (!open) setDeleteTarget(null); }}
      >
        <DialogContent className="sm:max-w-[420px] bg-sf border-border/40">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">
              Delete {deleteTarget?.name}?
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <p className="text-xs text-muted-foreground leading-relaxed">
              Every snapshot in{' '}
              <span className="font-medium text-foreground">{deleteTarget?.name}</span> is removed
              from the timeline. This cannot be undone.
            </p>
            {deleteReplacement !== undefined && (
              <div className="flex gap-3 p-3 rounded-lg bg-warn/10 border border-warn/20">
                <AlertTriangle size={16} className="text-warn shrink-0 mt-0.5" />
                <p className="text-xs text-muted-foreground leading-relaxed">
                  <span className="text-warn font-bold">Important:</span> this is the slot you are
                  playing. Your save files will be replaced with{' '}
                  <span className="text-warn font-bold">{deleteReplacement.name}</span>, so please
                  close the game first.
                </p>
              </div>
            )}
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="ghost" onClick={() => { setDeleteTarget(null); }}>Cancel</Button>
            <Button
              variant="destructive"
              disabled={busy}
              className="font-bold"
              onClick={() => { void handleDelete(); }}
            >
              Delete Slot
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-[400px] bg-sf border-border/40">
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">Create Slot</DialogTitle>
          </DialogHeader>
          <Input
            value={slotName}
            onChange={(event) => { setSlotName(event.target.value); }}
            placeholder="Slot name"
            className="bg-el border-border/40"
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                void handleCreate();
              }
            }}
            autoFocus
          />
          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              variant="ghost"
              onClick={() => {
                setCreateOpen(false);
                setSlotName('');
              }}
            >
              Cancel
            </Button>
            <Button
              disabled={slotName.trim() === '' || creating}
              onClick={() => { void handleCreate(); }}
            >
              {creating ? 'Creating...' : 'Create'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

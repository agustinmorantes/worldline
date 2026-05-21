import { ReactElement, useMemo, useState } from 'react';
import { Plus } from 'lucide-react';
import { Slot } from '@/models/slot.ts';
import { Button } from './ui/button';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from './ui/dialog';
import { Input } from './ui/input';

type SlotSelectorProps = {
  slots: Slot[];
  activeSlotId?: number;
  disabled?: boolean;
  onSwitch: (slot: Slot) => Promise<void>;
  onCreate: (name: string) => Promise<void>;
};

export function SlotSelector({
  slots,
  activeSlotId,
  disabled = false,
  onSwitch,
  onCreate,
}: SlotSelectorProps): ReactElement {
  const [createOpen, setCreateOpen] = useState(false);
  const [slotName, setSlotName] = useState('');
  const [busySlotId, setBusySlotId] = useState<number | null>(null);
  const [creating, setCreating] = useState(false);

  const sortedSlots = useMemo(
    () => [...slots].sort((left, right) => left.id - right.id),
    [slots],
  );

  async function handleSwitch(slot: Slot): Promise<void> {
    if (disabled || busySlotId !== null || creating || slot.id === activeSlotId) return;

    try {
      setBusySlotId(slot.id);
      await onSwitch(slot);
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
    } finally {
      setCreating(false);
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
              <Button
                key={slot.id}
                variant={isActive ? 'default' : 'secondary'}
                size="sm"
                disabled={disabled || creating || isBusy}
                className="h-8 shrink-0 rounded-md px-3 text-xs font-semibold"
                onClick={() => { void handleSwitch(slot); }}
              >
                {isBusy ? 'Switching...' : slot.name}
              </Button>
            );
          })}
        </div>

        <Button
          variant="ghost"
          size="sm"
          className="h-8 shrink-0 rounded-md px-3 text-xs font-semibold"
          disabled={disabled || creating || busySlotId !== null}
          onClick={() => { setCreateOpen(true); }}
        >
          <Plus size={14} className="mr-1.5" />
          New Slot
        </Button>
      </div>

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

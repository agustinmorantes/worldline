// "Manage playthroughs…": every playthrough in a list, to rename any of them without switching
// to it, since switching writes the live saves.
import { type ReactElement, type RefObject, useRef, useState } from 'react';
import { Pencil } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Slot } from '@/ipc/bindings/Slot';
import { renameSlot } from '@/ipc/commands';
import { utcDate } from '@/ipc/time';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { btn, field } from '@/shared/ui/classes';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';

export function ManagePlaythroughs({ game, open, onOpenChange, opener }: {
  game: FullGame;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Where focus goes back to on close: the dialog opens from a menu that is gone by then. */
  opener: RefObject<HTMLElement | null>;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const summaries = new Map(game.slot_summaries.map((s) => [s.slot_id, s]));
  /** The playthrough whose name is being edited. */
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState('');
  // The field can blur as it goes away after Enter or Esc; this keeps that from saving again.
  const editingRef = useRef<number | null>(null);
  /** After Enter or Esc, the row's Rename button takes focus back from the field as it returns. */
  const refocusRef = useRef<number | null>(null);

  function edit(slot: Slot | null): void {
    editingRef.current = slot?.id ?? null;
    setEditing(slot?.id ?? null);
    if (slot !== null) setDraft(slot.name);
  }

  async function commit(slot: Slot, keyboard: boolean): Promise<void> {
    if (editingRef.current !== slot.id) return;
    edit(null);
    if (keyboard) refocusRef.current = slot.id;
    const next = draft.trim();
    if (next === '' || next === slot.name) return;
    try {
      await renameSlot(slot.id, next);
      void queryClient.invalidateQueries({ queryKey: ['game', game.id] });
      void queryClient.invalidateQueries({ queryKey: ['games'] });
    } catch (err) {
      reportError(t.game.failed.renameSlot, err);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => { edit(null); onOpenChange(o); }}>
      {/* Esc leaves the name field first, then the dialog. */}
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] grid-rows-[auto_minmax(0,1fr)_auto] bg-sf sm:max-w-[440px]"
        onEscapeKeyDown={(e) => { if (editing !== null) e.preventDefault(); }}
        onCloseAutoFocus={(e) => { e.preventDefault(); opener.current?.focus(); }}
      >
        <DialogHeader>
          <DialogTitle className="text-lg font-bold tracking-tight">{t.game.playthroughs}</DialogTitle>
          <DialogDescription className="text-tx3">{t.game.managePlaythroughsHint}</DialogDescription>
        </DialogHeader>
        <ul className="-mx-1 flex min-h-0 min-w-0 flex-col gap-2 overflow-y-auto px-1">
          {game.slots.map((slot) => {
            const summary = summaries.get(slot.id);
            const on = slot.id === game.active_slot?.id;
            const meta = summary === undefined || summary.version_count === 0 || summary.last_created_at === null
              ? t.game.emptySlot
              : t.game.slotMeta(summary.version_count, formatRelative(utcDate(summary.last_created_at), locale));
            return (
              <li key={slot.id} className="flex min-h-[52px] items-center gap-2.5 rounded-xl bg-el py-1.5 pl-3 pr-1.5">
                <span aria-hidden="true" className="size-2 shrink-0 rounded-full" style={{ background: on ? 'var(--acc)' : 'var(--tx3)' }} />
                {editing === slot.id ? (
                  <form className="min-w-0 flex-1" onSubmit={(e) => { e.preventDefault(); void commit(slot, true); }}>
                    <input
                      className={`${field} h-9 bg-sf`}
                      value={draft}
                      aria-label={t.game.playthroughNameLabel}
                      autoFocus
                      onChange={(e) => { setDraft(e.target.value); }}
                      onBlur={() => { void commit(slot, false); }}
                      onKeyDown={(e) => {
                        if (e.key === 'Escape') { refocusRef.current = slot.id; edit(null); }
                      }}
                    />
                  </form>
                ) : (
                  <>
                    <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-sm font-semibold">{slot.name}</span>
                      <span className="text-xs font-medium text-tx3">{on ? t.game.currentSlot(meta) : meta}</span>
                    </span>
                    <button
                      type="button"
                      ref={(el) => {
                        if (el === null || refocusRef.current !== slot.id) return;
                        refocusRef.current = null;
                        el.focus();
                      }}
                      aria-label={t.game.renamePlaythrough(slot.name)}
                      className="flex size-8 shrink-0 items-center justify-center rounded-full text-tx3 transition-colors hover:bg-ln hover:text-tx"
                      onClick={() => { edit(slot); }}
                    >
                      <Pencil size={14} strokeWidth={2.4} aria-hidden="true" />
                    </button>
                  </>
                )}
              </li>
            );
          })}
        </ul>
        <DialogFooter>
          <button type="button" className={btn} onClick={() => { onOpenChange(false); }}>{t.game.done}</button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

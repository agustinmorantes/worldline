// "Manage playthroughs…": every playthrough in a list, to rename or delete any of them without
// switching to it, since switching writes the live saves. Deleting the current one asks which
// playthrough to switch to first.
import { type ReactElement, type ReactNode, type RefObject, useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Pencil, Trash2 } from 'lucide-react';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Slot } from '@/ipc/bindings/Slot';
import { renameSlot } from '@/ipc/commands';
import { utcDate } from '@/ipc/time';
import { queryClient } from '@/app/query-client';
import { reportError } from '@/shared/errors/report';
import { formatRelative, useLocale, useT } from '@/shared/i18n';
import { spring, useMotionReduced } from '@/shared/motion/motion';
import { btn, btnQuiet, field } from '@/shared/ui/classes';
import { Tooltip } from '@/shared/ui/Tooltip';
import { cn } from '@/shared/ui/utils';
import { slotName } from './game-view';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';

/** Which button of which row takes the focus once the list has settled. */
interface Refocus { slotId: number | null; action: 'delete' | 'confirm' }

export function ManagePlaythroughs({ game, open, onOpenChange, opener, running, busy, onDelete }: {
  game: FullGame;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Where focus goes back to on close: the dialog opens from a menu that is gone by then. */
  opener: RefObject<HTMLElement | null>;
  /** The game is running: the current playthrough can't be deleted, since that switches the saves. */
  running: boolean;
  /** Something is writing the save folder (or deleting) right now. */
  busy: boolean;
  /** Deletes `slot`, switching to `replacement` first when it is the current one. False when it failed. */
  onDelete: (slot: Slot, replacement: number | undefined) => Promise<boolean>;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const reduced = useMotionReduced();
  const first = t.game.playthroughName(1);
  const summaries = new Map(game.slot_summaries.map((s) => [s.slot_id, s]));
  /** The playthrough whose name is being edited. */
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState('');
  // The field can blur as it goes away after Enter or Esc; this keeps that from saving again.
  const editingRef = useRef<number | null>(null);
  /** After Enter or Esc, the row's Rename button takes focus back from the field as it returns. */
  const refocusRef = useRef<number | null>(null);
  /** The playthrough whose delete is being confirmed, while it is still in the list. */
  const [confirmingId, setConfirmingId] = useState<number | null>(null);
  const confirming = game.slots.find((s) => s.id === confirmingId) ?? null;
  /** The playthrough to switch to before deleting the current one: picked, never assumed. */
  const [replacement, setReplacement] = useState<number | null>(null);
  const [deleting, setDeleting] = useState(false);
  /** Where the focus goes once the list has rendered the change that moved it. */
  const refocusToRef = useRef<Refocus | null>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const doneRef = useRef<HTMLButtonElement>(null);

  // Focus follows the list: back to a row's Delete after Cancel or a failure, on to the next row
  // after a delete (or Done when none is left).
  useEffect(() => {
    const refocus = refocusToRef.current;
    if (refocus === null) return;
    refocusToRef.current = null;
    const target = refocus.slotId === null ? null
      : listRef.current?.querySelector<HTMLElement>(`[data-slot="${refocus.slotId.toString()}"] [data-action="${refocus.action}"]`);
    (target ?? doneRef.current)?.focus();
  });

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

  function confirm(slot: Slot): void {
    edit(null);
    setReplacement(null);
    setConfirmingId(slot.id);
  }

  function cancel(): void {
    if (confirming !== null) refocusToRef.current = { slotId: confirming.id, action: 'delete' };
    setConfirmingId(null);
  }

  /** Why `slot` can't be deleted now, if it can't. */
  function blocked(slot: Slot): string | null {
    if (game.slots.length <= 1) return t.game.lastSlot;
    if (running && slot.id === game.active_slot?.id) return t.game.closeToDelete(game.title);
    return null;
  }

  async function remove(slot: Slot): Promise<void> {
    const current = slot.id === game.active_slot?.id;
    if (deleting || busy || blocked(slot) !== null || (current && replacement === null)) return;
    const index = game.slots.findIndex((s) => s.id === slot.id);
    const neighbour = index + 1 < game.slots.length ? index + 1 : index - 1;
    const nextId = neighbour >= 0 && neighbour !== index ? game.slots[neighbour].id : null;
    setDeleting(true);
    const done = await onDelete(slot, current ? replacement ?? undefined : undefined);
    if (done) {
      refocusToRef.current = { slotId: nextId, action: 'delete' };
      setConfirmingId(null);
    } else {
      // The playthrough stays, usable again, with the focus back on its Delete.
      refocusToRef.current = { slotId: slot.id, action: 'confirm' };
    }
    setDeleting(false);
  }

  const iconButton = 'flex size-8 shrink-0 items-center justify-center rounded-full text-tx3 transition-[background-color,color,transform] duration-150 ease-spring hover:bg-ln hover:text-tx active:scale-[0.88]';

  return (
    <Dialog open={open} onOpenChange={(o) => { edit(null); setConfirmingId(null); onOpenChange(o); }}>
      {/* Esc leaves the name field or the delete question first, then the dialog. */}
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] grid-rows-[auto_minmax(0,1fr)_auto] bg-sf sm:max-w-[440px]"
        onEscapeKeyDown={(e) => {
          if (editing !== null) e.preventDefault();
          if (confirming !== null) {
            e.preventDefault();
            if (!deleting) cancel();
          }
        }}
        onCloseAutoFocus={(e) => { e.preventDefault(); opener.current?.focus(); }}
        // Undo in the delete toast brings the playthrough back into this list, so it stays open.
        onInteractOutside={(e) => { if (e.target instanceof Element && e.target.closest('[data-sonner-toaster]') !== null) e.preventDefault(); }}
      >
        <DialogHeader>
          <DialogTitle className="text-lg font-bold tracking-tight">{t.game.playthroughs}</DialogTitle>
          <DialogDescription className="text-tx3">{t.game.managePlaythroughsHint}</DialogDescription>
        </DialogHeader>
        <ul ref={listRef} className="-mx-1 flex min-h-0 min-w-0 flex-col gap-2 overflow-y-auto px-1">
          <AnimatePresence initial={false}>
            {game.slots.map((slot, i) => {
              const summary = summaries.get(slot.id);
              const on = slot.id === game.active_slot?.id;
              const name = slotName(slot.name, first);
              const meta = summary === undefined || summary.version_count === 0 || summary.last_created_at === null
                ? t.game.emptySlot
                : t.game.slotMeta(summary.version_count, formatRelative(utcDate(summary.last_created_at), locale));
              const why = blocked(slot);
              const asking = confirming?.id === slot.id;
              return (
                <motion.li
                  key={slot.id}
                  data-slot={slot.id}
                  layout={!reduced}
                  className="flex flex-col rounded-xl bg-el py-1.5 pl-3 pr-1.5"
                  // A restored playthrough rises back from Recently deleted; a deleted one sinks away toward it.
                  initial={reduced ? false : { opacity: 0, y: 6, scale: 0.96 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, y: 6, scale: 0.96, transition: spring }}
                  transition={reduced ? { duration: 0 } : { ...spring, delay: Math.min(i, 5) * 0.04, layout: spring }}
                >
                  <div className="flex min-h-[40px] items-center gap-2.5">
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
                          <span className="truncate text-sm font-semibold">{name}</span>
                          <span className="text-xs font-medium text-tx3">{on ? t.game.currentSlot(meta) : meta}</span>
                        </span>
                        <button
                          type="button"
                          ref={(el) => {
                            if (el === null || refocusRef.current !== slot.id) return;
                            refocusRef.current = null;
                            el.focus();
                          }}
                          aria-label={t.game.renamePlaythrough(name)}
                          className={iconButton}
                          onClick={() => { edit(slot); }}
                        >
                          <Pencil size={14} strokeWidth={2.4} aria-hidden="true" />
                        </button>
                        {/* Focusable while blocked, so the reason shows on keyboard focus too. */}
                        <Tooltip content={why ?? t.game.deletePlaythrough(name)}>
                          <button
                            type="button"
                            data-action="delete"
                            aria-label={t.game.deletePlaythrough(name)}
                            aria-disabled={why !== null || busy || asking}
                            aria-expanded={asking}
                            className={cn(iconButton, 'hover:text-err aria-disabled:pointer-events-auto aria-disabled:opacity-40 aria-disabled:hover:bg-transparent aria-disabled:hover:text-tx3')}
                            onClick={() => { if (why === null && !busy && !asking) confirm(slot); }}
                          >
                            <Trash2 size={14} strokeWidth={2.4} aria-hidden="true" />
                          </button>
                        </Tooltip>
                      </>
                    )}
                  </div>
                  <Unfold open={asking} label={t.game.deleteSlotTitle(name)}>
                    <DeleteQuestion
                      game={game}
                      slot={slot}
                      name={name}
                      current={on}
                      blocked={why}
                      busy={busy || deleting}
                      replacement={replacement}
                      onReplacement={setReplacement}
                      onCancel={cancel}
                      onDelete={() => { void remove(slot); }}
                    />
                  </Unfold>
                </motion.li>
              );
            })}
          </AnimatePresence>
        </ul>
        <DialogFooter>
          <button ref={doneRef} type="button" className={btn} onClick={() => { onOpenChange(false); }}>{t.game.done}</button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Unfolds out of its row, and folds back into it; scrolls itself into view once open. */
function Unfold({ open, label, children }: { open: boolean; label: string; children: ReactNode }): ReactElement {
  const reduced = useMotionReduced();
  const ref = useRef<HTMLDivElement>(null);
  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          key="unfold"
          ref={ref}
          role="group"
          aria-label={label}
          className="overflow-hidden"
          initial={reduced ? false : { height: 0, opacity: 0 }}
          animate={{ height: 'auto', opacity: 1 }}
          exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { height: 0, opacity: 0, transition: spring }}
          transition={reduced ? { duration: 0 } : spring}
          onAnimationComplete={() => { ref.current?.scrollIntoView({ block: 'nearest' }); }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function DeleteQuestion({ game, slot, name, current, blocked, busy, replacement, onReplacement, onCancel, onDelete }: {
  game: FullGame;
  slot: Slot;
  name: string;
  current: boolean;
  /** Why it can't be deleted now (the game started meanwhile), if it can't. */
  blocked: string | null;
  busy: boolean;
  replacement: number | null;
  onReplacement: (slotId: number) => void;
  onCancel: () => void;
  onDelete: () => void;
}): ReactElement {
  const t = useT();
  const first = t.game.playthroughName(1);
  const others = game.slots.filter((s) => s.id !== slot.id);
  const ready = blocked === null && !busy && (!current || replacement !== null);
  return (
    <div className="flex flex-col gap-3 pb-1.5 pr-1.5 pt-2">
      <div className="flex flex-col gap-1">
        <p className="text-sm font-semibold">{t.game.deleteSlotTitle(name)}</p>
        <p className="text-xs text-tx2">
          {current && `${t.game.deleteCurrentBody(name)} `}
          {t.game.deleteSlotBody}
        </p>
      </div>
      {current && (
        <fieldset className="flex flex-col gap-1">
          <legend className="pb-1 text-[11px] font-bold uppercase tracking-[0.12em] text-tx3">{t.game.switchTo}</legend>
          {others.map((other, i) => (
            <label key={other.id} className="flex min-h-9 cursor-pointer items-center gap-2.5 rounded-[10px] bg-sf px-2.5 text-sm font-semibold transition-colors duration-150 has-[:checked]:bg-ln">
              <input
                type="radio"
                name={`replacement-${slot.id.toString()}`}
                className="accent-[var(--acc)]"
                checked={replacement === other.id}
                autoFocus={i === 0}
                onChange={() => { onReplacement(other.id); }}
              />
              <span className="truncate">{slotName(other.name, first)}</span>
            </label>
          ))}
        </fieldset>
      )}
      {blocked !== null && <p className="text-xs font-medium text-err">{blocked}</p>}
      <div className="flex justify-end gap-2">
        <button type="button" className={btnQuiet} autoFocus={!current} onClick={onCancel}>{t.game.cancel}</button>
        <button
          type="button"
          data-action="confirm"
          aria-disabled={!ready}
          className={cn(btn, 'bg-err text-bg aria-disabled:opacity-40 aria-disabled:hover:translate-y-0 aria-disabled:hover:brightness-100')}
          onClick={() => { if (ready) onDelete(); }}
        >
          <Trash2 size={15} strokeWidth={2.4} aria-hidden="true" />
          {t.game.delete}
        </button>
      </div>
    </div>
  );
}

// The search in the sidebar's "Library" row. Wide, the field grows leftwards out of the icon
// over the eyebrow; on the rail it slides out of the sidebar's edge as a flyout, anchored at
// the icon, with the rail filtering live behind it.
import { type KeyboardEvent, type ReactElement, useEffect, useRef } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Search, X } from 'lucide-react';
import { useT } from '@/shared/i18n';
import { ease, spring } from '@/shared/motion/motion';
import { cn } from '@/shared/ui/utils';
import type { Fold } from './fold';

/** The wide field's width: the row from the nav's inset to the icon's right edge. */
const FIELD_WIDTH = 206;
const ICON_SIZE = 28;

/** Whether a key event comes from somewhere the user types. */
function inTextField(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || target.closest('input, textarea, select') !== null);
}

export function LibrarySearch({ fold, open, query, empty, onOpen, onQuery, onClose, onSubmit }: {
  fold: Fold;
  open: boolean;
  query: string;
  /** The query matches no game. */
  empty: boolean;
  onOpen: () => void;
  onQuery: (query: string) => void;
  onClose: () => void;
  /** Enter: open the first match. */
  onSubmit: () => void;
}): ReactElement {
  const t = useT();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Ctrl+F or "/" outside a text field.
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent): void => {
      if (inTextField(event.target)) return;
      const find = event.ctrlKey && event.key.toLowerCase() === 'f';
      const slash = event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey && !inTextField(event.target);
      if (!find && !slash) return;
      event.preventDefault();
      onOpen();
      inputRef.current?.focus();
    };
    window.addEventListener('keydown', onKey);
    return () => { window.removeEventListener('keydown', onKey); };
  }, [onOpen]);

  // The field mounts when it opens, and again in the other place when the rail folds or unfolds.
  useEffect(() => {
    const field = inputRef.current;
    if (!open || field === null) return;
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  }, [open, fold.collapsed]);

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>): void => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      buttonRef.current?.focus();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      onSubmit();
    }
  };

  const field = (
    <input
      ref={inputRef}
      type="text"
      role="searchbox"
      aria-label={t.shell.search}
      placeholder={t.shell.searchPlaceholder}
      value={query}
      onChange={(event) => { onQuery(event.target.value); }}
      onKeyDown={onKeyDown}
      onBlur={(event) => { if (query === '' && event.relatedTarget !== buttonRef.current && event.currentTarget.parentElement?.contains(event.relatedTarget) !== true) onClose(); }}
      spellCheck={false}
      autoComplete="off"
      className="h-full min-w-0 flex-1 bg-transparent text-[13px] font-medium text-tx outline-none placeholder:text-tx3"
    />
  );
  const close = (
    <button
      type="button"
      aria-label={t.shell.closeSearch}
      title={t.shell.closeSearch}
      onClick={() => { onClose(); buttonRef.current?.focus(); }}
      className="flex size-6 shrink-0 items-center justify-center rounded-md text-tx3 transition-[color,background-color,transform] duration-200 ease-spring hover:bg-white/[0.08] hover:text-tx active:scale-90"
    >
      <X size={14} strokeWidth={2.4} aria-hidden="true" />
    </button>
  );
  const instant = { duration: 0 };
  const inline = open && !fold.collapsed;

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        aria-label={t.shell.search}
        aria-expanded={open}
        title={t.shell.search}
        onClick={() => { if (open) onClose(); else onOpen(); }}
        className={cn(
          'absolute right-[22px] top-[14px] flex size-7 items-center justify-center rounded-lg text-tx3 transition-[color,background-color,opacity] duration-200 hover:bg-white/[0.05] hover:text-tx',
          // The wide field takes over the icon's spot; its own glyph stands where this one was.
          inline && 'pointer-events-none opacity-0',
        )}
      >
        <Search size={14} strokeWidth={2.4} aria-hidden="true" />
      </button>
      <AnimatePresence>
        {inline && (
          <motion.div
            key="field"
            className="absolute right-[22px] top-[14px] flex h-7 items-center gap-1.5 overflow-hidden rounded-lg bg-white/[0.06] pl-2 pr-7"
            initial={{ width: ICON_SIZE, opacity: 0 }}
            animate={{ width: FIELD_WIDTH, opacity: 1 }}
            exit={{ width: ICON_SIZE, opacity: 0, transition: fold.reduced ? instant : { width: { duration: 0.24, ease }, opacity: { duration: 0.12, delay: 0.14 } } }}
            transition={fold.reduced ? instant : { width: { type: 'spring', stiffness: 380, damping: 32 }, opacity: { duration: 0.12 } }}
          >
            <Search size={13} strokeWidth={2.4} aria-hidden="true" className="shrink-0 text-tx3" />
            {field}
            {/* The search icon turns into the cross that closes the field, in the same spot. */}
            <motion.span
              className="absolute right-0.5 flex"
              initial={{ rotate: -90, opacity: 0 }}
              animate={{ rotate: 0, opacity: 1 }}
              transition={fold.reduced ? instant : { duration: 0.24, ease }}
            >
              {close}
            </motion.span>
          </motion.div>
        )}
      </AnimatePresence>
      <AnimatePresence>
        {open && fold.collapsed && (
          <motion.div
            key="flyout"
            className="absolute left-full top-[8px] ml-2 w-[232px] rounded-xl bg-el p-1 shadow-[0_16px_40px_oklch(0%_0_0/0.5),inset_0_0_0_1px_oklch(100%_0_0/0.08)]"
            initial={{ opacity: 0, x: -14 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -14, transition: fold.reduced ? instant : { duration: 0.18, ease } }}
            transition={fold.reduced ? instant : { x: spring, opacity: { duration: 0.18 } }}
          >
            <div className="flex h-8 items-center gap-2 pl-2 pr-1">
              <Search size={14} strokeWidth={2.4} aria-hidden="true" className="shrink-0 text-tx2" />
              {field}
              {close}
            </div>
            <AnimatePresence initial={false}>
              {empty && (
                <motion.p
                  key="empty"
                  className="overflow-hidden text-xs text-tx3"
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={fold.reduced ? instant : { duration: 0.24, ease }}
                >
                  <span className="block px-2 pb-1.5 pt-0.5">{t.shell.noMatches}</span>
                </motion.p>
              )}
            </AnimatePresence>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

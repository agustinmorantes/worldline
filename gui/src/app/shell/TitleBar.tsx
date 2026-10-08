import { type ReactElement, useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { AnimatePresence, motion } from 'motion/react';
import { Minus, Square, X } from 'lucide-react';
import { useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { WorldlineGlyph } from '@/shared/ui/WorldlineGlyph';

export function TitleBar(): ReactElement {
  const t = useT();
  const control = 'flex h-9 w-11 items-center justify-center text-tx3 transition-colors duration-150 hover:bg-white/[0.06] hover:text-tx';

  return (
    <header data-tauri-drag-region className="relative z-[7] flex h-9 shrink-0 select-none items-center gap-[9px] bg-tint pl-3.5">
      <WorldlineGlyph />
      <span data-tauri-drag-region className="font-display text-sm font-bold tracking-[-0.02em]">Worldline</span>
      <span data-tauri-drag-region className="flex-1 self-stretch" />
      <HelpButton />
      <div className="flex">
        <button type="button" aria-label={t.shell.minimize} className={control} onClick={() => { void getCurrentWindow().minimize(); }}>
          <Minus size={14} />
        </button>
        <button type="button" aria-label={t.shell.maximize} className={control} onClick={() => { void getCurrentWindow().toggleMaximize(); }}>
          <Square size={11} />
        </button>
        <button
          type="button"
          aria-label={t.shell.close}
          className={`${control} hover:bg-err hover:text-bg`}
          onClick={() => { void getCurrentWindow().close(); }}
        >
          <X size={14} />
        </button>
      </div>
    </header>
  );
}

/** The `?` in the title bar. A stub until the help popover lands with the new onboarding. */
function HelpButton(): ReactElement {
  const t = useT();
  const [open, setOpen] = useState(false);
  const reduced = useMotionReduced();
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: Event): void => {
      if (event instanceof KeyboardEvent ? event.key === 'Escape' : rootRef.current?.contains(event.target as Node) !== true) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', close);
    document.addEventListener('keydown', close);
    return () => {
      document.removeEventListener('pointerdown', close);
      document.removeEventListener('keydown', close);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative mr-1">
      <button
        type="button"
        aria-label={t.shell.help}
        aria-expanded={open}
        className="flex size-6 items-center justify-center rounded-full font-display text-[13px] font-bold text-tx3 transition-[color,background-color,transform] duration-200 ease-spring hover:bg-white/[0.08] hover:text-tx active:scale-90"
        onClick={() => { setOpen((o) => !o); }}
      >
        ?
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            role="menu"
            className="absolute right-0 top-8 z-50 w-56 rounded-2xl bg-el p-1.5 shadow-[0_24px_60px_oklch(0%_0_0/0.55),inset_0_0_0_1px_oklch(100%_0_0/0.06)]"
            initial={{ opacity: 0, y: -6, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -6, scale: 0.97 }}
            transition={{ duration: reduced ? 0 : 0.22, ease }}
          >
            <button
              type="button"
              role="menuitem"
              className="flex min-h-10 w-full items-center rounded-[10px] px-2.5 text-sm font-semibold transition-colors duration-150 hover:bg-white/[0.06]"
              onClick={() => {
                setOpen(false);
                window.dispatchEvent(new CustomEvent('open-onboarding'));
              }}
            >
              {t.shell.replayTour}
            </button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

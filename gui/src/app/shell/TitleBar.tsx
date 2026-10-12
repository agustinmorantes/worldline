import type { ReactElement } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Square, X } from 'lucide-react';
import { HelpMenu } from '@/features/onboarding/HelpMenu';
import { useT } from '@/shared/i18n';
import { WorldlineGlyph } from '@/shared/ui/WorldlineGlyph';

// The open tour is a modal dialog, which sets `pointer-events: none` on the body; the title bar
// opts back in so the window can still be dragged, minimized, maximized and closed.
export function TitleBar(): ReactElement {
  const t = useT();
  const control = 'flex h-9 w-11 items-center justify-center text-tx3 transition-colors duration-150 hover:bg-white/[0.06] hover:text-tx';

  return (
    <header data-tauri-drag-region className="pointer-events-auto relative z-[7] flex h-9 shrink-0 select-none items-center gap-[9px] bg-tint pl-3.5">
      <WorldlineGlyph />
      <span data-tauri-drag-region className="font-display text-sm font-bold tracking-[-0.02em]">Worldline</span>
      <span data-tauri-drag-region className="flex-1 self-stretch" />
      <HelpMenu />
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

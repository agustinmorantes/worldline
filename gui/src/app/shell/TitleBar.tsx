import { ReactElement } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Save, Minus, Square, X } from 'lucide-react';

export function TitleBar(): ReactElement {
  return (
    <div className="relative z-[120] flex h-12 shrink-0 select-none">
      {/* Sidebar-aligned branding — same visual space as the sidebar panel */}
      <div className="w-[222px] bg-sidebar flex items-center gap-3 px-2">
        <div className="w-8 h-8 bg-primary rounded-lg flex items-center justify-center text-primary-foreground shadow-lg shadow-primary/20">
          <Save size={18} />
        </div>
        <span className="text-sm font-black tracking-tight text-foreground">WORLDLINE</span>
      </div>

      {/* Content-area top: drag region + window controls */}
      <div className="flex-1 flex border-b border-border/40">
        <div className="flex-1" data-tauri-drag-region />
        <div className="flex">
          <button
            type="button"
            aria-label="Minimize"
            className="flex items-center justify-center w-10 h-full text-muted-foreground hover:bg-white/5 transition-colors"
            onClick={() => { void getCurrentWindow().minimize(); }}
          >
            <Minus size={13} />
          </button>
          <button
            type="button"
            aria-label="Maximize"
            className="flex items-center justify-center w-10 h-full text-muted-foreground hover:bg-white/5 transition-colors"
            onClick={() => { void getCurrentWindow().toggleMaximize(); }}
          >
            <Square size={11} />
          </button>
          <button
            type="button"
            aria-label="Close"
            className="flex items-center justify-center w-10 h-full text-muted-foreground hover:bg-red-500 hover:text-white transition-colors"
            onClick={() => { void getCurrentWindow().close(); }}
          >
            <X size={13} />
          </button>
        </div>
      </div>
    </div>
  );
}

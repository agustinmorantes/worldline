// The in-game popup (ADR 0009): its sound, whether it shows, its default corner, quiet mode and
// the device-ahead warning. Device preferences, so they live in `prefs.json`.
import { useId, useRef, type KeyboardEvent, type ReactElement } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { useSyncSettings } from '@/features/library/queries';
import { useT } from '@/shared/i18n';
import { duration, ease, spring, useMotionReduced } from '@/shared/motion/motion';
import { setPref, usePrefs } from '@/shared/prefs/prefs';
import { CORNERS, type Corner, type OverlayPrefs } from '@/shared/prefs/schema';
import { Card, Toggle } from './controls';

export function OverlaySection(): ReactElement {
  const t = useT();
  const overlay = usePrefs().overlay;
  const { data: sync } = useSyncSettings();
  const syncOn = typeof sync?.remote_path === 'string' && sync.remote_path !== '';
  const reduced = useMotionReduced();
  const change = (next: Partial<OverlayPrefs>): void => { setPref('overlay', { ...overlay, ...next }); };

  return (
    <Card title={t.overlay.section}>
      <div className="mt-4 flex flex-col gap-3.5">
        {/* Covers the hotkeys too, so it stays when the popup is off. */}
        <Toggle label={t.overlay.sound} hint={t.overlay.soundHint} on={overlay.sound} onChange={(sound) => { change({ sound }); }} />
        <Toggle label={t.overlay.show} hint={t.overlay.showHint} on={overlay.enabled} onChange={(enabled) => { change({ enabled }); }} />
        <AnimatePresence initial={false}>
          {overlay.enabled && (
            <motion.div
              key="popup"
              className="-mt-3.5 overflow-hidden"
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={reduced ? { duration: 0 } : { duration: duration.base, ease }}
            >
              <div className="flex flex-col gap-3.5 pt-3.5">
                <CornerPicker value={overlay.corner} onChange={(corner) => { change({ corner }); }} />
                <Toggle label={t.overlay.quiet} hint={t.overlay.quietHint} on={overlay.quiet} onChange={(quiet) => { change({ quiet }); }} />
                {/* Only the cloud knows another device is ahead. */}
                {syncOn && (
                  <Toggle
                    label={t.overlay.warnAhead}
                    hint={t.overlay.warnAheadHint}
                    on={overlay.warnAhead}
                    onChange={(warnAhead) => { change({ warnAhead }); }}
                  />
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </Card>
  );
}

const PLACE: Record<Corner, string> = {
  tl: 'left-1.5 top-1.5',
  tr: 'right-1.5 top-1.5',
  bl: 'bottom-1.5 left-1.5',
  br: 'bottom-1.5 right-1.5',
};

/** The arrow keys move across the mini screen: left and right swap the side, up and down the edge. */
function across(corner: Corner, key: string): Corner | null {
  const [v, h] = corner.split('') as ['t' | 'b', 'l' | 'r'];
  if (key === 'ArrowLeft' || key === 'ArrowRight') return `${v}${key === 'ArrowLeft' ? 'l' : 'r'}`;
  if (key === 'ArrowUp' || key === 'ArrowDown') return `${key === 'ArrowUp' ? 't' : 'b'}${h}`;
  return null;
}

/** Four corners over a mini screen: the picked one shows a tiny popup, the others a placeholder. */
function CornerPicker({ value, onChange }: { value: Corner; onChange: (corner: Corner) => void }): ReactElement {
  const t = useT();
  const id = useId();
  const buttonsRef = useRef(new Map<Corner, HTMLButtonElement>());
  const keyDown = (e: KeyboardEvent): void => {
    const next = across(value, e.key);
    if (!next) return;
    e.preventDefault();
    onChange(next);
    buttonsRef.current.get(next)?.focus();
  };

  return (
    <div className="flex items-start gap-4">
      <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
        <span id={id} className="text-sm font-semibold">{t.overlay.corner}</span>
        <span className="text-[12.5px] text-tx3">{t.overlay.cornerHint}</span>
        <span className="mt-2 text-[12.5px] font-semibold text-tx2">{t.overlay.corners[value]}</span>
      </span>
      <div
        role="radiogroup"
        aria-labelledby={id}
        className="relative h-[135px] w-[240px] shrink-0 rounded-[10px] bg-[linear-gradient(160deg,oklch(26%_0.05_272),oklch(16%_0.035_260))] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.08)]"
        onKeyDown={keyDown}
      >
        {CORNERS.map((corner) => {
          const picked = corner === value;
          return (
            <button
              key={corner}
              ref={(el) => { if (el) buttonsRef.current.set(corner, el); else buttonsRef.current.delete(corner); }}
              type="button"
              role="radio"
              aria-checked={picked}
              aria-label={t.overlay.corners[corner]}
              tabIndex={picked ? 0 : -1}
              className={`absolute flex h-[30px] w-16 items-center justify-center rounded-[9px] transition-[background-color,transform] duration-200 ease-spring hover:bg-[oklch(100%_0_0/0.06)] active:scale-[0.92] ${PLACE[corner]}`}
              onClick={() => { onChange(corner); }}
            >
              {picked ? (
                <motion.span
                  layoutId={`${id}-pill`}
                  transition={spring}
                  aria-hidden="true"
                  className="flex h-[18px] items-center gap-[5px] rounded-[7px] bg-[oklch(10%_0.015_265/0.75)] pl-[3px] pr-[7px] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.14)]"
                >
                  <i className="size-3 rounded-[4px] bg-acc" />
                  <em className="h-1 w-7 rounded-[2px] bg-[oklch(100%_0_0/0.7)]" />
                </motion.span>
              ) : (
                <span aria-hidden="true" className="h-3.5 w-11 rounded-md border-[1.5px] border-dashed border-[oklch(100%_0_0/0.25)]" />
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

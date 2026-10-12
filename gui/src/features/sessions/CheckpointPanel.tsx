// The selected checkpoint: its screenshot, when and where it was made, its name, and the
// hold-to-rewind button.
import { type ReactElement, useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { ArrowDown, X } from 'lucide-react';
import { Dialog as DialogPrimitive } from 'radix-ui';
import type { FullGame } from '@/ipc/bindings/FullGame';
import type { Version } from '@/ipc/bindings/Version';
import { GameIcon } from '@/features/library/GameIcon';
import type { CheckpointDevice } from '@/features/game/game-view';
import { formatRelative, formatTime, useLocale, useT } from '@/shared/i18n';
import { duration, ease, spring, useMotionReduced } from '@/shared/motion/motion';
import { btn, btnQuiet } from '@/shared/ui/classes';
import { DeviceGlyph } from '@/shared/ui/DeviceGlyph';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/ui/dialog';
import { HoldButton } from '@/shared/ui/HoldButton';
import { formatDuration, markColor } from './Timeline';
import { useCleanupWouldRemove, useScreenshot } from './queries';
import { minutesUntil, timeOf } from './sessions';

export function CheckpointPanel({ game, version, active, unrecorded, onLine, device, lessPlayMs, aheadDevice, busy, running, onName, onRewind, onClose, now }: {
  game: FullGame;
  version: Version;
  /** The checkpoint you are on. */
  active: Version | null;
  /** The saves changed since `active` and wait to be recorded: you can go back to it. */
  unrecorded: { dueAt: Date } | null;
  /** On your current line: rewinding, not switching timelines. */
  onLine: boolean;
  device: CheckpointDevice;
  /** Play since this checkpoint, on your line. */
  lessPlayMs: number;
  /** Another device's progress that would move aside. */
  aheadDevice: string | null;
  busy: boolean;
  /** The game is running here, so it may hold or save over its files. */
  running: boolean;
  /** Saves the name; resolves whether it was saved. */
  onName: (tag: string) => Promise<boolean>;
  onRewind: () => void;
  /** Present when the panel is a drawer. */
  onClose?: () => void;
  now: Date;
}): ReactElement {
  const t = useT();
  const locale = useLocale();
  const reduced = useMotionReduced();
  const here = active?.id === version.id;
  const back = here && unrecorded !== null;
  const [draft, setDraft] = useState(version.tag ?? '');
  // Another checkpoint, or the name changed underneath: start the field over.
  const source = `${version.id.toString()}:${version.tag ?? ''}`;
  const [shown, setShown] = useState(source);
  const [unnaming, setUnnaming] = useState(false);
  // Each opening of the confirm dialog asks the next cleanup afresh.
  const [opening, setOpening] = useState(0);
  const fieldRef = useRef<HTMLInputElement>(null);
  const removeButtonRef = useRef<HTMLButtonElement>(null);
  // Where focus goes when the confirm dialog closes: back where it was asked from, or to the field
  // once the name (and with it the X button) is gone.
  const closeFocusRef = useRef<'button' | 'field'>('button');
  // The checkpoint and name a removal is on its way for: until it lands, leaving the empty field
  // mustn't ask again. Released when it ends, so a failed one can be asked again.
  const removingRef = useRef<string | null>(null);
  const sourceRef = useRef(source);
  useEffect(() => { sourceRef.current = source; });
  const removal = useCleanupWouldRemove(version.uuid, opening, unnaming);
  if (shown !== source) {
    setShown(source);
    setDraft(version.tag ?? '');
  }

  const date = new Date(timeOf(version));
  const tagged = version.tag !== null && version.tag !== '';
  const title = tagged ? (version.tag ?? '') : t.sessions.autosave;
  const eyebrow = back ? t.checkpoint.goBackTo : here ? t.checkpoint.youAreHere : onLine ? t.checkpoint.rewindTo : t.checkpoint.switchTo;
  /** The "Now" row: the checkpoint you are on, or the progress since it that isn't recorded yet. */
  const current = back
    ? { name: t.sessions.notRecorded, when: t.sessions.dueIn(minutesUntil(unrecorded.dueAt, now)) }
    : !here && active !== null ? { name: active.tag ?? t.sessions.autosave, when: formatRelative(new Date(timeOf(active)), locale, now) } : null;
  const fold = reduced ? { duration: 0 } : { duration: duration.base, ease };
  const tint = markColor({ side: !onLine, elsewhere: !device.here });

  function commit(): void {
    const next = draft.trim();
    if (next === (version.tag ?? '')) return;
    // Clearing the name by hand asks first, like the Remove name button.
    if (next === '' && tagged) {
      if (removingRef.current !== source) askUnname('field');
      return;
    }
    void onName(next);
  }

  function askUnname(from: 'button' | 'field'): void {
    closeFocusRef.current = from;
    setOpening((n) => n + 1);
    setUnnaming(true);
  }

  function keepName(): void {
    setDraft(version.tag ?? '');
    setUnnaming(false);
  }

  function unname(): void {
    const removing = source;
    const kept = version.tag ?? '';
    removingRef.current = removing;
    closeFocusRef.current = 'field';
    setUnnaming(false);
    void onName('').then((saved) => {
      removingRef.current = null;
      // The name is still there (the failure was reported): show it again.
      if (!saved && sourceRef.current === removing) setDraft(kept);
    });
  }

  return (
    <motion.div key={version.id} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.38, ease }}>
      {/* On the checkpoint you are on, the "Now" row and the hold button fold in and out with its unrecorded changes. */}
      <AnimatePresence initial={false}>
        {current !== null && (
          <motion.div key="now" className="overflow-hidden" initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={fold}>
            <div className="flex h-11 items-center gap-2.5 rounded-[14px] bg-sf px-3.5">
              <GameIcon title={game.title} hue={game.accent_hue} iconPath={game.icon_path ?? undefined} iconRevision={game.icon_sha256 ?? undefined} size={22} className="text-[9px]" />
              <span className="text-[13px] font-bold">{t.checkpoint.now}</span>
              <span className="min-w-0 flex-1 truncate text-[13px] text-tx2">{current.name}</span>
              <span className="font-mono text-xs text-tx3">{current.when}</span>
            </div>
            <div className="relative flex h-[46px] items-center">
              <span aria-hidden="true" className="absolute inset-y-1 left-6 border-l-2 border-dashed border-acc-line" />
              {onLine && lessPlayMs >= 60_000 && (
                <span className="ml-[42px] flex h-[26px] items-center gap-[7px] rounded-lg bg-acc-soft px-2.5 font-mono text-[12.5px] font-semibold text-acc">
                  <ArrowDown size={13} strokeWidth={2.6} aria-hidden="true" />
                  {t.checkpoint.lessPlay(formatDuration(lessPlayMs, t))}
                </span>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      <div className="overflow-hidden rounded-[20px] bg-sf shadow-[0_24px_48px_-24px_oklch(0%_0_0/0.6)]">
        <ScreenshotSlot versionId={version.id} tint={tint} />
        <div className="px-[18px] pb-[18px] pt-4">
          <div className="mb-1.5 flex items-baseline gap-2.5">
            <span className="text-[11px] font-bold uppercase tracking-[0.12em]" style={{ color: tint }}>{eyebrow}</span>
            <span className="ml-auto font-mono text-xs text-tx3">{t.checkpoint.number(version.version_number)}</span>
            {onClose !== undefined && (
              <button type="button" aria-label={t.checkpoint.close} onClick={onClose} className="-mr-1 self-center rounded-md p-1 text-tx3 hover:text-tx min-[1000px]:hidden">
                <X size={15} aria-hidden="true" />
              </button>
            )}
          </div>
          <div className="truncate font-display text-2xl font-extrabold">{title}</div>
          <div className="mt-1.5 flex items-center gap-[7px] text-[13px] text-tx2">
            <span className="font-mono">
              {formatTime(date, locale)} · {formatRelative(date, locale, now)}
            </span>
            <span aria-hidden="true" className="size-[3px] rounded-full bg-tx3" />
            <span className={device.here ? 'flex' : 'flex text-deck'}><DeviceGlyph kind={device.kind} size={14} /></span>
            <span className="truncate">{device.here ? t.checkpoint.thisDevice : (device.name ?? t.sessions.anotherDevice)}</span>
          </div>
          <label className="mt-3.5 flex h-10 items-center gap-2.5 rounded-xl bg-el px-3 text-sm text-acc transition-shadow duration-200 focus-within:shadow-[inset_0_0_0_1.5px_var(--acc-line)]">
            <span aria-hidden="true" className="size-2 shrink-0 rotate-45 bg-current" />
            <input
              ref={fieldRef}
              value={draft}
              onChange={(e) => { setDraft(e.target.value); }}
              onKeyDown={(e) => {
                // Its default would press whatever takes focus next, like the confirm dialog's Cancel.
                if (e.key === 'Enter') { e.preventDefault(); e.currentTarget.blur(); }
                if (e.key === 'Escape') { setDraft(version.tag ?? ''); e.stopPropagation(); }
              }}
              onBlur={commit}
              placeholder={t.checkpoint.namePlaceholder}
              aria-label={t.checkpoint.nameLabel}
              className="min-w-0 flex-1 bg-transparent font-semibold text-tx outline-none placeholder:font-normal placeholder:text-tx3"
            />
            {tagged && (
              <button
                type="button"
                ref={removeButtonRef}
                aria-label={t.checkpoint.removeName}
                onClick={() => { askUnname('button'); }}
                className="-mr-1.5 flex size-7 shrink-0 items-center justify-center rounded-full text-tx3 transition-colors hover:bg-ln hover:text-tx"
              >
                <X size={14} strokeWidth={2.6} aria-hidden="true" />
              </button>
            )}
          </label>
        </div>
      </div>
      <Dialog open={unnaming} onOpenChange={(open) => { if (!open) keepName(); }}>
        <DialogContent
          className="bg-sf sm:max-w-[420px]"
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            (closeFocusRef.current === 'button' ? removeButtonRef : fieldRef).current?.focus();
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-lg font-bold tracking-tight">{t.checkpoint.removeNameTitle(version.tag ?? '')}</DialogTitle>
            <DialogDescription className="text-tx2">{t.checkpoint.removeNameBody}</DialogDescription>
          </DialogHeader>
          <div aria-live="polite">
            <AnimatePresence initial={false}>
              {(removal.isPending || removal.isError || removal.data) && (
                <motion.p
                  key="cleanup"
                  className="overflow-hidden text-sm"
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={fold}
                >
                  {removal.isPending
                    ? <span className="text-tx3">{t.checkpoint.removeNameChecking}</span>
                    : removal.isError
                      ? <span className="text-tx3">{t.checkpoint.removeNameUnchecked}</span>
                      : <span className="font-semibold text-warn">{t.checkpoint.removeNameNextCleanup}</span>}
                </motion.p>
              )}
            </AnimatePresence>
          </div>
          <DialogFooter className="gap-2">
            <button type="button" className={btnQuiet} onClick={keepName}>{t.game.cancel}</button>
            <button type="button" className={btn} onClick={unname}>{t.checkpoint.removeName}</button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <AnimatePresence initial={false}>
        {(!here || back) && (
          <motion.div key="hold" className="overflow-hidden" initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={fold}>
            <HoldButton
              label={back ? t.checkpoint.holdToGoBack : onLine ? t.checkpoint.holdToRewind : t.checkpoint.holdToSwitch}
              hint={t.checkpoint.holdHint}
              disabled={busy || running}
              onComplete={onRewind}
              rumble
            />
            {back && <div className="mt-3 text-center text-xs text-tx2">{t.checkpoint.keptFirst}</div>}
            {running ? (
              <div className="mt-3 flex items-center gap-2 text-[13px] font-semibold text-warn">
                <span aria-hidden="true" className="size-[7px] shrink-0 rounded-full bg-warn" />
                {t.checkpoint.running(game.title)}
              </div>
            ) : (
              <div className="mt-3 text-center text-xs text-tx3">{t.checkpoint.closeFirst}</div>
            )}
            {aheadDevice !== null && (
              <div className="mt-3 flex items-center gap-2 text-[13px] font-semibold text-warn">
                <span aria-hidden="true" className="size-[7px] shrink-0 rounded-full bg-warn" />
                {t.checkpoint.aheadWarning(aheadDevice)}
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}

/** The screenshot's size in the panel: its full 336 px width, at 16:9. */
const SHOT = { width: 336, height: 189 };
/** The tint line under it, and all of the slot while there is no screenshot. */
const LINE = 4;

/** The enlarged screenshot's margin from the window's edges. */
const ZOOM_MARGIN = 32;

interface Box { left: number; top: number; width: number; height: number }

function boxOf(el: HTMLElement): Box {
  const { left, top, width, height } = el.getBoundingClientRect();
  return { left, top, width, height };
}

/**
 * Where the enlarged screenshot sits: at its own aspect, as large as the window allows inside the
 * margin but never past its natural size, centred.
 */
export function zoomBox(natural: { width: number; height: number }, view: { width: number; height: number }): Box {
  const scale = Math.min(
    1,
    (view.width - 2 * ZOOM_MARGIN) / natural.width,
    (view.height - 2 * ZOOM_MARGIN) / natural.height,
  );
  const width = natural.width * scale;
  const height = natural.height * scale;
  return { left: (view.width - width) / 2, top: (view.height - height) / 2, width, height };
}

/**
 * The tint line on top of the card. When the checkpoint's screenshot exists, the line grows
 * into it once the image has loaded, and the image settles in above the line. Clicking it lifts
 * the image out of the card and grows it to fill the window; a click anywhere or Esc settles it
 * back into the card.
 */
export function ScreenshotSlot({ versionId, tint }: { versionId: number; tint: string }): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const { data: path } = useScreenshot(versionId);
  const src = typeof path === 'string' ? convertFileSrc(path) : null;
  /** The source that finished loading; a broken file never opens the slot. */
  const [loaded, setLoaded] = useState<string | null>(null);
  const open = src !== null && loaded === src;
  const transition = reduced ? { duration: 0 } : { duration: duration.slow, ease };
  const thumbRef = useRef<HTMLImageElement>(null);
  /**
   * Enlarged: the image's place in the card and in the window. While `closing` it flies back
   * to the card, and only then is it gone and the card's image shown again.
   */
  const [zoom, setZoom] = useState<{ from: Box; to: Box; closing: boolean } | null>(null);
  // Another checkpoint, or the screenshot gone: nothing stays enlarged.
  const [zoomed, setZoomed] = useState(src);
  if (zoomed !== src) {
    setZoomed(src);
    setZoom(null);
  }

  function enlarge(): void {
    const img = thumbRef.current;
    if (img === null || img.naturalWidth === 0) return;
    const view = { width: window.innerWidth, height: window.innerHeight };
    setZoom({ from: boxOf(img), to: zoomBox({ width: img.naturalWidth, height: img.naturalHeight }, view), closing: false });
  }

  function settle(): void {
    const img = thumbRef.current;
    // Back to where the card's image is now, in case the panel moved meanwhile.
    setZoom((z) => (z === null ? null : { ...z, from: img === null ? z.from : boxOf(img), closing: true }));
  }

  return (
    <motion.div
      className="relative overflow-hidden"
      initial={false}
      animate={{ height: open ? SHOT.height : LINE }}
      transition={transition}
    >
      {/* The tint stays under the image's bottom edge: behind the card's rounded corners it
          would bleed through their antialiasing. */}
      <span aria-hidden="true" className="absolute inset-x-0 bottom-0 transition-[background] duration-500" style={{ height: LINE, background: tint }} />
      {/* Cleanup can take the screenshot away; it fades as the slot folds back into the line. */}
      <AnimatePresence initial={false}>
        {src !== null && (
          <motion.img
            ref={thumbRef}
            key={src}
            src={src}
            alt={t.checkpoint.screenshot}
            width={SHOT.width}
            height={SHOT.height - LINE}
            loading="lazy"
            decoding="async"
            draggable={false}
            aria-hidden={!open}
            onLoad={() => { setLoaded(src); }}
            className="absolute inset-x-0 top-0 w-full object-cover"
            style={{ height: SHOT.height - LINE, visibility: zoom === null ? 'visible' : 'hidden' }}
            initial={false}
            animate={open ? { opacity: 1, scale: 1 } : { opacity: 0, scale: 1.04 }}
            exit={{ opacity: 0 }}
            transition={transition}
          />
        )}
      </AnimatePresence>
      {open && (
        <button
          type="button"
          aria-label={t.checkpoint.enlargeScreenshot}
          onClick={enlarge}
          className="absolute inset-x-0 top-0 cursor-zoom-in focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-acc"
          style={{ height: SHOT.height - LINE }}
        />
      )}
      <DialogPrimitive.Root open={zoom !== null} onOpenChange={(next) => { if (!next) settle(); }}>
        {zoom !== null && src !== null && (
          <DialogPrimitive.Portal>
            <DialogPrimitive.Overlay asChild>
              <motion.div
                className="pointer-events-auto fixed inset-0 z-50 bg-black/70"
                initial={{ opacity: 0 }}
                animate={{ opacity: zoom.closing ? 0 : 1 }}
                transition={transition}
              />
            </DialogPrimitive.Overlay>
            <DialogPrimitive.Content
              aria-describedby={undefined}
              onClick={settle}
              className="pointer-events-auto fixed inset-0 z-50 cursor-zoom-out outline-none"
            >
              <DialogPrimitive.Title className="sr-only">{t.checkpoint.screenshot}</DialogPrimitive.Title>
              <motion.img
                src={src}
                alt={t.checkpoint.screenshot}
                draggable={false}
                className="fixed max-w-none object-cover shadow-[0_24px_64px_-16px_oklch(0%_0_0/0.7)]"
                initial={{ ...zoom.from, borderRadius: 0 }}
                animate={zoom.closing ? { ...zoom.from, borderRadius: 0 } : { ...zoom.to, borderRadius: 14 }}
                transition={reduced ? { duration: 0 } : spring}
                onAnimationComplete={() => { if (zoom.closing) setZoom(null); }}
              />
            </DialogPrimitive.Content>
          </DialogPrimitive.Portal>
        )}
      </DialogPrimitive.Root>
    </motion.div>
  );
}

// The tour: a card over the app that plays Lantern Knight, a 30-second demo game, with a real
// Worldline strip under it. It opens on first launch and from the `?` in the title bar, and
// closing shrinks it back into that `?`.
import { type KeyboardEvent, type ReactElement, type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from '@tanstack/react-router';
import { Dialog } from 'radix-ui';
import { AnimatePresence, motion, type TargetAndTransition } from 'motion/react';
import { ArrowRight, Play, Search } from 'lucide-react';
import { Choice } from '@/features/settings/controls';
import { useT } from '@/shared/i18n';
import { ease, useMotionReduced } from '@/shared/motion/motion';
import { getPrefs, setPref, usePrefs } from '@/shared/prefs/prefs';
import { HoldButton } from '@/shared/ui/HoldButton';
import { btn, eyebrow } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';
import { Kbd, WithHelpKey } from './Kbd';
import { FlyingSave, type Flying, Scene, Strip } from './LanternKnight';
import { arrowStep, CHEST_X, type Demo, demoAction, demoAt, demoLine, GATE_HERO_X, LESSONS, STOPS, STRIP_X, type TourStep } from './tour';
import { HELP_ANCHOR, type TourControl, TourContext } from './tour-context';
import './tour.css';

/** The card's canvas; it scales down to fit small windows. */
const CARD = { width: 960, height: 620 } as const;
/** The title bar the card sits under. */
const TITLE_BAR = 36;
const SHRINK_MS = 460;
/** How long the `?` stays lit with its tip after the tour closes. */
const TIP_MS = 3700;

interface Session { key: number; step: TourStep; fromHelp: boolean }

/** Holds the tour's open state for the whole app and draws the tour over it. */
export function TourHost({ children }: { children: ReactNode }): ReactElement {
  const router = useRouter();
  const reduced = useMotionReduced();
  // First launch only: the flag is set as the tour opens, so quitting midway doesn't bring it back.
  const [session, setSession] = useState<Session | null>(() => (getPrefs().onboardingSeen ? null : { key: 0, step: 0, fromHelp: false }));
  const [tip, setTip] = useState(false);
  const keyRef = useRef(0);
  const timersRef = useRef<number[]>([]);

  useEffect(() => {
    if (!getPrefs().onboardingSeen) setPref('onboardingSeen', true);
  }, []);

  useEffect(() => () => { timersRef.current.forEach((t) => { clearTimeout(t); }); }, []);

  const clearTimers = useCallback((): void => {
    timersRef.current.forEach((t) => { clearTimeout(t); });
    timersRef.current = [];
  }, []);

  const openAt = useCallback((step: TourStep): void => {
    clearTimers();
    setTip(false);
    setSession({ key: ++keyRef.current, step, fromHelp: true });
  }, [clearTimers]);

  const close = useCallback((findGames: boolean): void => {
    clearTimers();
    setSession(null);
    if (findGames) void router.navigate({ to: '/add', search: { scan: true } });
    timersRef.current.push(window.setTimeout(() => {
      setTip(true);
      timersRef.current.push(window.setTimeout(() => { setTip(false); }, TIP_MS));
    }, reduced ? 0 : SHRINK_MS));
  }, [clearTimers, router, reduced]);

  const dismissTip = useCallback((): void => {
    clearTimers();
    setTip(false);
  }, [clearTimers]);

  const control = useMemo<TourControl>(() => ({ open: session !== null, openAt, tip, dismissTip }), [session, openAt, tip, dismissTip]);

  return (
    <TourContext value={control}>
      {children}
      <Dialog.Root open={session !== null} onOpenChange={(open) => { if (!open) close(false); }}>
        <AnimatePresence>
          {session !== null && <TourCard key={session.key} start={session.step} fromHelp={session.fromHelp} onClose={close} />}
        </AnimatePresence>
      </Dialog.Root>
    </TourContext>
  );
}

interface Box { left: number; top: number; width: number; height: number; fit: number }

/** Where the card sits: centered under the title bar, scaled down if the window is small. */
function cardBox(): Box {
  const room = window.innerHeight - TITLE_BAR;
  const fit = Math.min(1, (window.innerWidth - 48) / CARD.width, (room - 32) / CARD.height);
  const width = CARD.width * fit;
  const height = CARD.height * fit;
  return { left: (window.innerWidth - width) / 2, top: TITLE_BAR + (room - height) / 2, width, height, fit };
}

function useCardBox(): Box {
  const [box, setBox] = useState(cardBox);
  useEffect(() => {
    const resize = (): void => { setBox(cardBox()); };
    window.addEventListener('resize', resize);
    return () => { window.removeEventListener('resize', resize); };
  }, []);
  return box;
}

/** The card squeezed into the title bar's `?`. */
function inHelpButton(box: Box): TargetAndTransition {
  const help = document.querySelector(`[${HELP_ANCHOR}]`)?.getBoundingClientRect();
  if (help === undefined) return { opacity: 0 };
  return {
    x: help.left + help.width / 2 - (box.left + box.width / 2),
    y: help.top + help.height / 2 - (box.top + box.height / 2),
    scaleX: help.width / box.width,
    scaleY: help.height / box.height,
    opacity: 0,
  };
}

function focusFirst(within: HTMLElement | null): void {
  within?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
}

const ghost =
  'inline-flex h-11 shrink-0 items-center whitespace-nowrap rounded-[14px] bg-white/5 px-[18px] text-[15px] font-bold text-tx2 transition-[background-color,color,transform] duration-200 ease-spring hover:bg-white/[0.09] hover:text-tx active:scale-95';
const primary = cn(btn, 'h-11 gap-2.5 rounded-[14px] px-5 text-[15px] aria-disabled:pointer-events-none aria-disabled:opacity-55');

function TourCard({ start, fromHelp, onClose }: {
  start: TourStep;
  fromHelp: boolean;
  onClose: (findGames: boolean) => void;
}): ReactElement {
  const t = useT();
  const reduced = useMotionReduced();
  const box = useCardBox();
  const [demo, setDemo] = useState<Demo>(() => demoAt(start));
  const [flying, setFlying] = useState<Flying | null>(null);
  const timersRef = useRef<number[]>([]);
  const contentRef = useRef<HTMLDivElement>(null);
  const primaryRef = useRef<HTMLDivElement>(null);
  const shownStepRef = useRef<TourStep>(start);

  useEffect(() => () => { timersRef.current.forEach((id) => { clearTimeout(id); }); }, []);

  const later = (ms: number, fn: () => void): void => { timersRef.current.push(window.setTimeout(fn, ms)); };
  const patch = (changes: Partial<Demo>): void => { setDemo((d) => ({ ...d, ...changes })); };

  function go(step: TourStep): void {
    if (step === demo.step) return;
    timersRef.current.forEach((id) => { clearTimeout(id); });
    timersRef.current = [];
    setFlying(null);
    setDemo(demoAt(step));
  }

  /** The game writes its save; a copy flies down onto the strip. */
  function saveTo(x: number, bad: boolean, done: () => void): void {
    patch({ walking: false, saving: true });
    later(380, () => { setFlying({ x, bad }); });
    later(820, () => { patch({ saving: false }); setFlying(null); done(); });
  }

  function play(): void {
    if (demo.busy || demo.saved >= 3) return;
    const walk = (i: number): void => {
      patch({ heroX: STOPS[i], walking: true, busy: true });
      later(700, () => {
        saveTo(STRIP_X[i] ?? 0, false, () => {
          patch({ saved: i + 1, busy: i < 2 });
          if (i < 2) later(160, () => { walk(i + 1); });
        });
      });
    };
    walk(demo.saved);
  }

  function openChest(): void {
    if (demo.mimic || demo.mimicSaved) return;
    patch({ mimic: true });
    later(900, () => { saveTo(CHEST_X, true, () => { patch({ mimicSaved: true }); }); });
  }

  function rewind(): void {
    patch({ rewinding: true });
    later(1000, () => { patch({ rewinding: false, rewound: true, mimic: false }); });
  }

  function walkOn(): void {
    if (!demo.rewound || demo.walkedOn || demo.busy) return;
    patch({ heroX: GATE_HERO_X, walking: true, busy: true });
    later(800, () => { saveTo(CHEST_X, false, () => { patch({ walkedOn: true, busy: false }); }); });
  }

  const lesson = demo.step >= 1 && demo.step <= 3;
  const action = lesson ? demoAction(demo) : null;
  const actionKey = action === null ? '' : action.kind === 'hold' ? `hold:${String(action.ready)}` : action.kind;

  // A new step puts the keyboard on its main button; within a step, so does a button that
  // replaced the one that had focus.
  useEffect(() => {
    const active = document.activeElement;
    const lost = active === null || active === document.body || active === contentRef.current;
    if (shownStepRef.current !== demo.step || lost) {
      focusFirst(primaryRef.current);
    }
    shownStepRef.current = demo.step;
  }, [demo.step, actionKey]);

  function onKeyDown(event: KeyboardEvent): void {
    const step = arrowStep(demo.step, event.key);
    if (step === null) return;
    event.preventDefault();
    go(step);
  }

  const shrunk = inHelpButton(box);
  const fast = reduced ? { duration: 0 } : null;

  return (
    <Dialog.Portal forceMount>
      <Dialog.Overlay forceMount asChild>
        <motion.div
          className="fixed inset-x-0 bottom-0 top-9 z-[60] bg-[oklch(8%_0.01_265/0.55)] backdrop-blur-[6px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1, transition: fast ?? { duration: 0.42 } }}
          exit={{ opacity: 0, transition: fast ?? { duration: SHRINK_MS / 1000 } }}
        />
      </Dialog.Overlay>
      <Dialog.Content
        forceMount
        asChild
        aria-describedby={undefined}
        onOpenAutoFocus={(e) => {
          e.preventDefault();
          focusFirst(primaryRef.current);
        }}
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          document.querySelector<HTMLElement>(`[${HELP_ANCHOR}]`)?.focus();
        }}
        onInteractOutside={(e) => { e.preventDefault(); }}
        onKeyDown={onKeyDown}
      >
        <motion.div
          ref={contentRef}
          className="fixed z-[61] overflow-hidden rounded-[28px] bg-sf/95 shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.08),0_40px_120px_-20px_oklch(0%_0_0/0.7)] outline-none"
          style={{ left: box.left, top: box.top, width: box.width, height: box.height }}
          initial={fromHelp ? shrunk : { opacity: 0, y: 18, scaleX: 0.97, scaleY: 0.97 }}
          animate={{
            x: 0,
            y: 0,
            scaleX: 1,
            scaleY: 1,
            opacity: 1,
            transition: fast ?? (fromHelp ? { duration: 0.54, ease } : { duration: 0.6, ease, delay: 0.12 }),
          }}
          exit={{ ...shrunk, transition: fast ?? { duration: SHRINK_MS / 1000, ease: [0.6, 0, 0.35, 1] } }}
        >
          <Dialog.Title className="sr-only">{t.tour.label}</Dialog.Title>
          <motion.div
            className="relative origin-top-left"
            style={{ width: CARD.width, height: CARD.height, scale: box.fit }}
            initial={fromHelp ? { opacity: 0 } : false}
            animate={{ opacity: 1, transition: fast ?? { duration: 0.26, delay: 0.32 } }}
            exit={{ opacity: 0, transition: fast ?? { duration: 0.15 } }}
          >
            {demo.step === 0 && (
              <Intro primaryRef={primaryRef} onStart={() => { go(1); }} onSkip={() => { onClose(false); }} />
            )}
            {lesson && (
              <>
                <Pills step={demo.step} onGo={go} />
                <button
                  type="button"
                  className="absolute right-7 top-7 flex h-8 items-center gap-2.5 rounded-[10px] pl-3 pr-1.5 text-[13px] font-semibold text-tx3 transition-colors duration-150 hover:bg-white/5 hover:text-tx"
                  onClick={() => { onClose(false); }}
                >
                  {t.tour.skip}
                  <Kbd>Esc</Kbd>
                </button>
                <Lesson demo={demo} />
                <div ref={primaryRef} className="absolute left-10 top-[372px] min-h-[52px]">
                  {action?.kind === 'hold' ? (
                    <HoldButton
                      label={t.checkpoint.holdToRewind}
                      hint={t.checkpoint.holdHint}
                      disabled={!action.ready}
                      onComplete={rewind}
                      className="mt-0 h-[52px] w-[280px] rounded-[14px]"
                    />
                  ) : action !== null && (
                    <ActionButton
                      action={action}
                      onPlay={play}
                      onChest={openChest}
                      onWalk={walkOn}
                      onNext={() => { go((demo.step + 1) as TourStep); }}
                    />
                  )}
                </div>
                <div className="absolute bottom-[30px] left-10 flex items-center gap-1.5 text-xs text-tx3">
                  <Kbd>←</Kbd>
                  <Kbd>→</Kbd>
                  <span className="ml-1 mr-3">{t.tour.steps}</span>
                  <Kbd>Space</Kbd>
                  <span className="ml-1">{t.tour.doIt}</span>
                </div>
                <Scene demo={demo} />
                <Strip demo={demo} />
                {flying !== null && <FlyingSave save={flying} />}
              </>
            )}
            {demo.step === 4 && (
              <Finish primaryRef={primaryRef} onFindGames={() => { onClose(true); }} onSkip={() => { onClose(false); }} />
            )}
          </motion.div>
        </motion.div>
      </Dialog.Content>
    </Dialog.Portal>
  );
}

const rise = {
  initial: { opacity: 0, y: 14, scale: 0.98 },
  animate: { opacity: 1, y: 0, scale: 1 },
  transition: { duration: 0.52, ease },
};

function Intro({ primaryRef, onStart, onSkip }: {
  primaryRef: React.RefObject<HTMLDivElement | null>;
  onStart: () => void;
  onSkip: () => void;
}): ReactElement {
  const t = useT();
  const { language } = usePrefs();
  return (
    <>
      <div className="absolute right-7 top-7">
        <Choice
          label={t.tour.language}
          value={language}
          // Language names stay in their own language.
          options={[
            { value: 'system', label: t.settings.system },
            { value: 'en', label: 'English' },
            { value: 'es', label: 'Español' },
          ]}
          onChange={(next) => { setPref('language', next); }}
        />
      </div>
      <motion.div {...rise} className="absolute inset-x-0 top-24 flex flex-col items-center text-center">
        <Mark size={132} />
        <h1 className="mt-9 font-display text-[44px] font-extrabold leading-[1.05] tracking-[-0.02em]">{t.tour.introTitle}</h1>
        <p className="mt-4 max-w-[520px] text-[17px] leading-[1.55] text-tx2">{t.tour.introBody}</p>
        <div ref={primaryRef} className="mt-8 flex gap-3">
          <button type="button" className={primary} onClick={onStart}>
            {t.tour.showMe}
            <ArrowRight size={16} strokeWidth={2.6} aria-hidden="true" />
          </button>
          <button type="button" className={ghost} onClick={onSkip}>{t.tour.skipIntro}</button>
        </div>
        <div className="mt-10 flex items-center gap-2 text-[13px] text-tx3"><WithHelpKey text={t.tour.replayLater} /></div>
      </motion.div>
    </>
  );
}

function Finish({ primaryRef, onFindGames, onSkip }: {
  primaryRef: React.RefObject<HTMLDivElement | null>;
  onFindGames: () => void;
  onSkip: () => void;
}): ReactElement {
  const t = useT();
  const label = 'lk-label-in absolute flex h-6 items-center gap-1.5 whitespace-nowrap rounded-[7px] px-[9px] text-[12.5px] font-bold';
  return (
    <motion.div {...rise} className="absolute inset-x-0 top-20 flex flex-col items-center text-center">
      <div className="relative h-40 w-[340px]">
        <div className="absolute left-[90px] top-0">
          <Mark size={160} branch />
        </div>
        <span className={cn(label, 'left-[262px] top-[33px] bg-acc-soft text-acc')} style={{ '--d': '950ms' } as React.CSSProperties}>
          <span aria-hidden="true" className="size-1.5 rotate-45 bg-current" />{t.tour.checkpoints.gate}
        </span>
        <span className={cn(label, 'left-[262px] top-[103px] bg-el text-tx2')} style={{ '--d': '1050ms' } as React.CSSProperties}>
          <span aria-hidden="true" className="size-1.5 rotate-45 bg-current" />{t.tour.checkpoints.mimicRun}
        </span>
        <span className={cn(label, '-left-3.5 top-[68px] bg-acc-soft text-acc')} style={{ '--d': '1150ms' } as React.CSSProperties}>
          <span aria-hidden="true" className="size-1.5 rotate-45 bg-current" />{t.tour.checkpoints.chest}
        </span>
      </div>
      <h1 className="mt-9 font-display text-[44px] font-extrabold leading-[1.05] tracking-[-0.02em]">{t.tour.finishTitle}</h1>
      <p className="mt-4 max-w-[500px] text-[17px] leading-[1.55] text-tx2">{t.tour.finishBody}</p>
      <div ref={primaryRef} className="mt-8 flex gap-3">
        <button type="button" className={primary} onClick={onFindGames}>
          <Search size={16} strokeWidth={2.6} aria-hidden="true" />
          {t.tour.findGames}
        </button>
        <button type="button" className={ghost} onClick={onSkip}>{t.tour.addMyself}</button>
      </div>
      <div className="mt-9 flex items-center gap-2 text-[13px] text-tx3"><WithHelpKey text={t.tour.refresher} /></div>
    </motion.div>
  );
}

/** The app mark drawing itself; on the finish, the lower branch is the grey mimic run. */
function Mark({ size, branch = false }: { size: number; branch?: boolean }): ReactElement {
  const other = branch ? 'var(--tx3)' : 'var(--acc)';
  const d = (ms: number): React.CSSProperties => ({ '--d': `${String(branch ? ms - 50 : ms)}ms` } as React.CSSProperties);
  return (
    <div
      className="overflow-hidden bg-[linear-gradient(165deg,oklch(28%_0.06_290),oklch(16%_0.03_265))] shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.08),0_0_60px_oklch(60%_0.2_290/0.25)]"
      style={{ width: size, height: size, borderRadius: size * 0.225 }}
    >
      <svg viewBox="0 0 64 64" width={size} height={size} aria-hidden="true" fill="none" strokeLinecap="round" className="block drop-shadow-[0_0_8px_oklch(80%_0.13_290/0.45)]">
        <path className="lk-draw" pathLength={1} d="M10 32 L26 32" stroke="var(--acc)" strokeWidth={3.5} style={d(200)} />
        <path className="lk-draw" pathLength={1} d="M26 32 C 36 32 40 18 52 18" stroke="var(--acc)" strokeWidth={3.5} style={d(500)} />
        <path className="lk-draw" pathLength={1} d="M26 32 C 36 32 40 46 52 46" stroke={other} strokeWidth={3.5} style={d(500)} />
        <circle className="lk-pop origin-center [transform-box:fill-box]" cx="52" cy="18" r="3.4" fill="var(--acc)" style={d(840)} />
        <circle className="lk-pop origin-center [transform-box:fill-box]" cx="52" cy="46" r="3.4" fill={other} style={d(840)} />
        <circle className="lk-pop origin-center [transform-box:fill-box]" cx="26" cy="32" r="6" fill="var(--acc)" stroke="oklch(21% .04 290)" strokeWidth={2.7} style={d(460)} />
      </svg>
    </div>
  );
}

function Pills({ step, onGo }: { step: TourStep; onGo: (step: TourStep) => void }): ReactElement {
  const t = useT();
  return (
    <div className="absolute left-8 top-7 flex items-center gap-1.5">
      {LESSONS.map((n) => {
        const on = n === step;
        const done = n < step;
        return (
          <div key={n} className="contents">
            {n > 1 && <span aria-hidden="true" className="h-0.5 w-3.5 rounded-sm bg-ln" />}
            <button
              type="button"
              aria-current={on ? 'step' : undefined}
              className={cn(
                'flex h-8 items-center gap-2 rounded-full pl-[5px] pr-[13px] text-[13px] font-bold transition-[background-color,color,transform] duration-300 ease-spring hover:text-tx active:scale-[0.94]',
                on ? 'bg-acc-soft text-acc' : done ? 'text-tx2' : 'text-tx3',
              )}
              onClick={() => { onGo(n); }}
            >
              <span
                aria-hidden="true"
                className={cn(
                  'flex size-[22px] items-center justify-center rounded-full font-mono text-[11px] font-bold transition-colors duration-300',
                  on ? 'bg-acc text-acc-ink' : done ? 'bg-acc-soft text-acc' : 'bg-el',
                )}
              >
                {done ? '✓' : n}
              </span>
              {t.tour.lessons[n]}
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** The lesson's text. It lives in a live region, so each line is read out as the demo moves. */
function Lesson({ demo }: { demo: Demo }): ReactElement {
  const t = useT();
  const step = demo.step as 1 | 2 | 3;
  const line = demoLine(demo);
  return (
    <div aria-live="polite" className="absolute left-10 top-28 w-[330px]">
      <motion.div key={step} initial={{ opacity: 0, x: -16 }} animate={{ opacity: 1, x: 0 }} transition={{ duration: 0.46, ease }}>
        <div className={cn(eyebrow, 'text-acc')}>{t.tour.lessonOf(step, t.tour.lessons[step])}</div>
        <h2 className="mt-3 font-display text-[34px] font-extrabold leading-[1.08] tracking-[-0.02em]">{t.tour.titles[step]}</h2>
      </motion.div>
      <motion.p
        key={`${String(step)}:${String(line)}`}
        className="mt-4 min-h-[104px] text-base leading-[1.6] text-tx2"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.38, ease }}
      >
        {t.tour.lines[step][line]}
      </motion.p>
    </div>
  );
}

function ActionButton({ action, onPlay, onChest, onWalk, onNext }: {
  action: Exclude<ReturnType<typeof demoAction>, { kind: 'hold' }>;
  onPlay: () => void;
  onChest: () => void;
  onWalk: () => void;
  onNext: () => void;
}): ReactElement {
  const t = useT();
  const arrow = <ArrowRight size={16} strokeWidth={2.4} aria-hidden="true" />;
  // Busy stays focusable (aria-disabled) so the keyboard isn't dropped mid-walk.
  const [label, icon, busy, run] = ((): [string, ReactElement, boolean, () => void] => {
    switch (action.kind) {
      case 'play': return [action.busy ? t.tour.playing : t.tour.play, <Play key="play" size={16} fill="currentColor" aria-hidden="true" />, action.busy, onPlay];
      case 'chest': return [t.tour.openChest, <ChestIcon key="chest" />, false, onChest];
      case 'walk': return [action.busy ? t.tour.walking : t.tour.walkOn, arrow, action.busy, onWalk];
      case 'next': return [action.finish ? t.tour.finish : t.tour.next, arrow, false, onNext];
    }
  })();
  return (
    <button type="button" className={primary} aria-disabled={busy || undefined} onClick={() => { if (!busy) run(); }}>
      {icon}
      {label}
    </button>
  );
}

function ChestIcon(): ReactElement {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 11h16v8a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1zM4 11V8a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3v3M12 11v3" />
    </svg>
  );
}

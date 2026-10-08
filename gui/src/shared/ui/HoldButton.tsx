// Big jumps (rewinding, deleting everywhere) need the button held for a moment, with the
// pointer or with Space/Enter, so a stray click never does them.
import { type ReactElement, type ReactNode, useEffect, useRef, useState } from 'react';
import { animate, motion, useMotionValue, useTransform } from 'motion/react';
import { RotateCcw } from 'lucide-react';
import { ease, spring, useMotionReduced } from '@/shared/motion/motion';
import { cn } from '@/shared/ui/utils';

export const HOLD_MS = 900;

/** Rumble ceiling: translate (px) and rotation (deg) at full charge, and how often the jitter moves. */
const RUMBLE = { px: 1.5, deg: 0.4, hz: 35 } as const;

export function HoldButton({ label, hint, disabled, onComplete, icon, danger = false, rumble = false, className }: {
  label: string;
  hint: string;
  disabled: boolean;
  onComplete: () => void;
  icon?: ReactNode;
  /** Fills in the error color instead of the accent. */
  danger?: boolean;
  /** Vibrates harder as the fill charges, then pops on completion (rewind). */
  rumble?: boolean;
  className?: string;
}): ReactElement {
  const reduced = useMotionReduced();
  const [holding, setHolding] = useState(false);
  const timerRef = useRef<number | undefined>(undefined);
  /** 0–1: the one source for the fill and the rumble, so the rumble drains with the fill. */
  const progress = useMotionValue(0);
  const fill = useTransform(progress, (p) => `${(p * 100).toString()}%`);
  const x = useMotionValue(0);
  const y = useMotionValue(0);
  const rotate = useMotionValue(0);
  const scale = useMotionValue(1);
  /** Set on completion: the stored energy is spent, so the drain after it doesn't rumble. */
  const releasedRef = useRef(false);

  useEffect(() => () => { window.clearTimeout(timerRef.current); }, []);

  useEffect(() => {
    if (!rumble || reduced) return;
    let lastJolt = 0;
    let jolt = { x: 0, y: 0, deg: 0 };
    return progress.on('change', (p) => {
      // Ease-in: about nothing for the first third, the full rumble just before completion.
      const amp = releasedRef.current ? 0 : p ** 3;
      const now = performance.now();
      if (now - lastJolt >= 1000 / RUMBLE.hz) {
        lastJolt = now;
        const angle = Math.random() * 2 * Math.PI;
        const reach = 0.5 + Math.random() * 0.5;
        jolt = { x: Math.cos(angle) * reach, y: Math.sin(angle) * reach, deg: Math.random() * 2 - 1 };
      }
      x.set(jolt.x * RUMBLE.px * amp);
      y.set(jolt.y * RUMBLE.px * amp);
      rotate.set(jolt.deg * RUMBLE.deg * amp);
    });
  }, [rumble, reduced, progress, x, y, rotate]);

  function drain(): void {
    void animate(progress, 0, { duration: 0.22, ease });
  }

  function start(): void {
    if (disabled || holding) return;
    setHolding(true);
    releasedRef.current = false;
    // Linear: a timer the user watches fill.
    void animate(progress, 1, { duration: HOLD_MS / 1000, ease: 'linear' });
    timerRef.current = window.setTimeout(() => {
      setHolding(false);
      releasedRef.current = true;
      drain();
      // The release: one quick swell past full size, settling back on the spring.
      if (rumble && !reduced) void animate(scale, 1, { ...spring, velocity: 2 });
      onComplete();
    }, HOLD_MS);
  }

  function stop(): void {
    if (!holding) return;
    window.clearTimeout(timerRef.current);
    setHolding(false);
    drain();
  }

  return (
    <motion.button
      type="button"
      disabled={disabled}
      aria-description={hint}
      onPointerDown={(e) => { if (e.button === 0) start(); }}
      onPointerUp={stop}
      onPointerLeave={stop}
      onPointerCancel={stop}
      onKeyDown={(e) => {
        if (e.key !== ' ' && e.key !== 'Enter') return;
        e.preventDefault();
        if (!e.repeat) start();
      }}
      onKeyUp={(e) => { if (e.key === ' ' || e.key === 'Enter') stop(); }}
      onBlur={stop}
      style={{ x, y, rotate, scale }}
      className={cn(
        // `scale` only: the rumble writes `transform` every frame and must not be smoothed away.
        'relative mt-3.5 flex h-14 w-full items-center justify-center overflow-hidden rounded-2xl bg-el text-[15px] font-bold transition-[scale] duration-[180ms] ease-spring active:scale-[0.98] disabled:opacity-60',
        danger ? 'text-err shadow-[inset_0_0_0_1.5px_oklch(68%_0.19_25/0.45)]' : 'shadow-[inset_0_0_0_1.5px_var(--acc-line)]',
        className,
      )}
    >
      <motion.span
        aria-hidden="true"
        className={cn('absolute inset-y-0 left-0', danger ? 'bg-err/30' : 'bg-acc-fill')}
        style={{ width: fill }}
      />
      <span className="relative flex items-center gap-2.5">
        {icon ?? <RotateCcw size={18} strokeWidth={2.4} aria-hidden="true" />}
        {label}
      </span>
    </motion.button>
  );
}

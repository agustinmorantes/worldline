// Rewinding throws away nothing, but it is a big jump: the button has to be held for a moment,
// with the pointer or with Space/Enter, so a stray click never moves you.
import { type ReactElement, useEffect, useRef, useState } from 'react';
import { RotateCcw } from 'lucide-react';
import { cn } from '@/shared/ui/utils';

export const HOLD_MS = 900;

export function HoldButton({ label, hint, disabled, onComplete }: {
  label: string;
  hint: string;
  disabled: boolean;
  onComplete: () => void;
}): ReactElement {
  const [holding, setHolding] = useState(false);
  const timerRef = useRef<number | undefined>(undefined);

  useEffect(() => () => { window.clearTimeout(timerRef.current); }, []);

  function start(): void {
    if (disabled || holding) return;
    setHolding(true);
    timerRef.current = window.setTimeout(() => {
      setHolding(false);
      onComplete();
    }, HOLD_MS);
  }

  function stop(): void {
    window.clearTimeout(timerRef.current);
    setHolding(false);
  }

  return (
    <button
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
      className="relative mt-3.5 flex h-14 w-full items-center justify-center overflow-hidden rounded-2xl bg-el text-[15px] font-bold shadow-[inset_0_0_0_1.5px_var(--acc-line)] transition-transform duration-[180ms] ease-spring active:scale-[0.98] disabled:opacity-60"
    >
      <span
        aria-hidden="true"
        className={cn('absolute inset-y-0 left-0 bg-acc-fill', holding ? 'w-full' : 'w-0')}
        style={{ transition: holding ? `width ${HOLD_MS.toString()}ms linear` : 'width 220ms var(--ez)' }}
      />
      <span className="relative flex items-center gap-2.5">
        <RotateCcw size={18} strokeWidth={2.4} aria-hidden="true" />
        {label}
      </span>
    </button>
  );
}

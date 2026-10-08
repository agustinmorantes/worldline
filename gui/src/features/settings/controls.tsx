// The controls the Settings sections share: a card, steppers, a switch and a segmented choice.
import { type ReactElement, type ReactNode, useId } from 'react';
import { motion } from 'motion/react';
import { spring } from '@/shared/motion/motion';
import { useT } from '@/shared/i18n';
import { eyebrow } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';

export function Card({ title, children }: { title: string; children: ReactNode }): ReactElement {
  const id = useId();
  return (
    <section aria-labelledby={id} className="rounded-[18px] bg-sf p-5">
      <h2 id={id} className={eyebrow}>{title}</h2>
      {children}
    </section>
  );
}

function Label({ id, label, hint }: { id?: string; label: string; hint?: string }): ReactElement {
  return (
    <span className="flex min-w-0 flex-1 flex-col gap-[3px]">
      <span id={id} className="text-sm font-semibold">{label}</span>
      {hint !== undefined && <span className="text-[12.5px] text-tx3">{hint}</span>}
    </span>
  );
}

/** A labelled value with − and + buttons; a side is disabled at its end. */
export function Stepper({ label, hint, value, onStep, canLess = true, canMore = true }: {
  label: string;
  hint?: string;
  value: string;
  onStep: (dir: 1 | -1) => void;
  canLess?: boolean;
  canMore?: boolean;
}): ReactElement {
  const t = useT();
  const button = 'flex size-[30px] items-center justify-center rounded-[9px] text-tx2 transition-[background-color,color,transform] duration-150 ease-spring hover:bg-ln hover:text-tx active:scale-[0.88] disabled:pointer-events-none disabled:opacity-35';
  return (
    <div role="group" aria-label={label} className="flex items-center gap-4">
      <Label label={label} hint={hint} />
      <span className="flex shrink-0 items-center gap-1 rounded-xl bg-el p-[3px]">
        <button type="button" aria-label={t.settings.less} className={button} disabled={!canLess} onClick={() => { onStep(-1); }}>−</button>
        <output aria-live="polite" className="w-[72px] text-center font-mono text-[12.5px]">{value}</output>
        <button type="button" aria-label={t.settings.more} className={button} disabled={!canMore} onClick={() => { onStep(1); }}>+</button>
      </span>
    </div>
  );
}

export function Toggle({ label, hint, on, onChange }: {
  label: string;
  hint?: string;
  on: boolean;
  onChange: (on: boolean) => void;
}): ReactElement {
  const id = useId();
  return (
    <div className="flex items-center gap-4">
      <Label id={id} label={label} hint={hint} />
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-labelledby={id}
        className={cn('relative h-6 w-10 shrink-0 rounded-full transition-colors duration-200', on ? 'bg-acc' : 'bg-ln')}
        onClick={() => { onChange(!on); }}
      >
        <motion.span
          aria-hidden="true"
          className={cn('absolute top-[3px] size-[18px] rounded-full', on ? 'bg-acc-ink' : 'bg-tx2')}
          initial={false}
          animate={{ left: on ? 19 : 3 }}
          transition={spring}
        />
      </button>
    </div>
  );
}

export interface ChoiceOption<T extends string> {
  value: T;
  label: string;
  /** Shown before the label. */
  icon?: ReactNode;
}

/** One of a few options, as a row of pills; the accent pill slides to the picked one. */
export function Choice<T extends string>({ label, options, value, onChange }: {
  label: string;
  options: ChoiceOption<T>[];
  value: T;
  onChange: (value: T) => void;
}): ReactElement {
  const id = useId();
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      <Label id={id} label={label} />
      <div role="radiogroup" aria-labelledby={id} className="flex h-[34px] shrink-0 gap-0.5 rounded-full bg-el p-[3px]">
        {options.map((option) => {
          const picked = option.value === value;
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={picked}
              className="relative flex items-center gap-1.5 rounded-full px-3 text-[13px] font-semibold text-tx2 transition-[color,transform] duration-200 ease-spring hover:text-tx active:scale-95 aria-checked:text-acc-ink"
              onClick={() => { onChange(option.value); }}
            >
              {picked && <motion.span layoutId={id} aria-hidden="true" className="absolute inset-0 rounded-full bg-acc" transition={spring} />}
              {option.icon !== undefined && <span className="relative flex">{option.icon}</span>}
              <span className="relative">{option.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

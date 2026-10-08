// The title bar's `?`: replay the tour or jump to one of its steps. Opens with `?` or F1 too.
import { type ReactElement, useEffect, useState } from 'react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import { Play } from 'lucide-react';
import { useT } from '@/shared/i18n';
import { eyebrow, popover, row } from '@/shared/ui/classes';
import { cn } from '@/shared/ui/utils';
import { WithHelpKey } from './Kbd';
import { LESSONS } from './tour';
import { HELP_ANCHOR, useTour } from './tour-context';

function typing(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName));
}

export function HelpMenu(): ReactElement {
  const t = useT();
  const tour = useTour();
  const [open, setOpen] = useState(false);
  const { open: touring, tip, dismissTip } = tour;

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (touring || typing(event.target) || (event.key !== '?' && event.key !== 'F1')) return;
      event.preventDefault();
      dismissTip();
      setOpen((o) => !o);
    };
    window.addEventListener('keydown', onKey);
    return () => { window.removeEventListener('keydown', onKey); };
  }, [touring, dismissTip]);

  return (
    <div className="relative mr-1">
      <Menu.Root open={open} onOpenChange={(next) => { if (next) dismissTip(); setOpen(next); }}>
        <Menu.Trigger
          {...{ [HELP_ANCHOR]: '' }}
          aria-label={t.shell.help}
          className={cn(
            'relative flex size-6 items-center justify-center rounded-full font-display text-[13px] font-bold text-tx3 transition-[color,background-color,transform] duration-200 ease-spring hover:bg-white/[0.08] hover:text-tx active:scale-90 data-[state=open]:bg-white/[0.08] data-[state=open]:text-tx',
            tip && 'bg-acc-soft text-acc',
          )}
        >
          {tip && <span aria-hidden="true" className="pointer-events-none absolute inset-0 rounded-full border-2 border-acc [animation:halo_1.8s_ease-out_infinite]" />}
          ?
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content align="end" sideOffset={8} className={cn(popover, 'w-[280px]')}>
            <Menu.Item className={cn(row, 'items-start')} onSelect={() => { tour.openAt(0); }}>
              <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg bg-acc-soft text-acc">
                <Play size={13} fill="currentColor" aria-hidden="true" />
              </span>
              <span className="flex flex-col">
                {t.help.replay}
                <span className="text-xs font-medium text-tx3">{t.help.replaySub}</span>
              </span>
            </Menu.Item>
            <Menu.Separator className="mx-2.5 my-1.5 h-px bg-ln" />
            <Menu.Label className={cn(eyebrow, 'px-2.5 pb-1 pt-1')}>{t.help.jump}</Menu.Label>
            {LESSONS.map((n) => (
              <Menu.Item key={n} className={row} onSelect={() => { tour.openAt(n); }}>
                <span aria-hidden="true" className="flex size-[22px] shrink-0 items-center justify-center rounded-full bg-el font-mono text-[11px] font-bold text-tx2 shadow-[inset_0_0_0_1px_oklch(100%_0_0/0.08)]">
                  {n}
                </span>
                <span className="flex flex-col">
                  {t.tour.lessons[n]}
                  <span className="text-xs font-medium text-tx3">{t.help.lessonSubs[n]}</span>
                </span>
              </Menu.Item>
            ))}
            <div className="mt-1 flex items-center gap-1.5 px-2.5 pb-1 pt-2 text-xs text-tx3">
              <WithHelpKey text={t.help.pressKey} />
            </div>
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
      {tip && (
        <div
          role="status"
          className="lk-tip absolute right-0 top-8 z-50 flex w-max items-center gap-1.5 rounded-xl bg-el px-3 py-2 text-[13px] font-semibold text-tx shadow-[0_16px_40px_oklch(0%_0_0/0.5),inset_0_0_0_1px_oklch(100%_0_0/0.08)]"
        >
          <WithHelpKey text={t.help.tip} />
        </div>
      )}
    </div>
  );
}

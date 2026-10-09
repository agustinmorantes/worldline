// The app's tooltip, in place of the native `title` one: a small inverted label on hover or
// keyboard focus, portaled so scrolling lists never clip it.
import type { ReactElement, ReactNode } from 'react';
import { Tooltip as TooltipPrimitive } from 'radix-ui';
import { cn } from './utils';

/** Once one tooltip is open, its neighbours open at once; they close as the pointer leaves. */
export function TooltipProvider({ children }: { children: ReactNode }): ReactElement {
  return (
    <TooltipPrimitive.Provider delayDuration={300} skipDelayDuration={400} disableHoverableContent>
      {children}
    </TooltipPrimitive.Provider>
  );
}

export function Tooltip({ content, label, side = 'top', mono = false, children }: {
  /** Empty shows the child alone. */
  content: ReactNode;
  /** What assistive tech reads when `content` is more than text; Radix would render it twice. */
  label?: string;
  side?: 'top' | 'right' | 'bottom' | 'left';
  /** Times and paths, set like the text they stand for. */
  mono?: boolean;
  /** One element that takes a ref and DOM props, such as a button. */
  children: ReactElement;
}): ReactElement {
  if (content === undefined || content === null || content === '') return children;
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger
        asChild
        // Keyboard focus shows it; focus that follows a click, or that the app moves, does not.
        onFocus={(event) => { if (!event.currentTarget.matches(':focus-visible')) event.preventDefault(); }}
      >
        {children}
      </TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          collisionPadding={8}
          aria-label={label}
          className={cn(
            'z-[60] max-w-[min(360px,calc(100vw-16px))] origin-(--radix-tooltip-content-transform-origin) rounded-lg bg-tx px-2 py-1 text-xs font-semibold text-bg shadow-[0_12px_32px_oklch(0%_0_0/0.45)] [overflow-wrap:anywhere] animate-in fade-in-0 zoom-in-95 duration-150 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95',
            mono && 'font-mono',
          )}
        >
          {content}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

import type { ReactElement, ReactNode } from 'react';
import { cn } from '@/shared/ui/utils';

/** A keycap. */
export function Kbd({ children, round = false }: { children: ReactNode; round?: boolean }): ReactElement {
  return (
    <kbd
      className={cn(
        'inline-flex h-[22px] min-w-[22px] items-center justify-center rounded-md bg-el px-1.5 font-mono text-[11px] font-semibold text-tx2 shadow-[inset_0_-1.5px_0_oklch(0%_0_0/0.35)]',
        round && 'w-[22px] rounded-full px-0',
      )}
    >
      {children}
    </kbd>
  );
}

/** Copy with a `{key}` placeholder, drawn as the `?` keycap. */
export function WithHelpKey({ text }: { text: string }): ReactElement {
  const [before, after = ''] = text.split('{key}');
  return <>{before}<Kbd round>?</Kbd>{after}</>;
}

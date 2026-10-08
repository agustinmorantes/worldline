// The sidebar folds into an icon rail below 1100 px. The width springs only when the
// breakpoint is crossed; labels fade and clip on the way, and leave the layout (but not
// the accessibility tree) once the rail has settled.
import { useSyncExternalStore } from 'react';
import { cn } from '@/shared/ui/utils';

const NARROW = '(max-width: 1099px)';

function subscribe(onChange: () => void): () => void {
  const query = window.matchMedia(NARROW);
  query.addEventListener('change', onChange);
  return () => { query.removeEventListener('change', onChange); };
}

/** Whether the window is below the rail breakpoint. Changes only when the breakpoint is crossed. */
export function useNarrow(): boolean {
  return useSyncExternalStore(subscribe, () => window.matchMedia(NARROW).matches);
}

export interface Fold {
  /** The rail is narrow, or narrowing. */
  collapsed: boolean;
  /** The rail has finished narrowing: labels are out of the layout. */
  hidden: boolean;
  reduced: boolean;
}

/**
 * A label that fades out as the rail narrows and back in once it has widened. Once folded it is
 * `sr-only` (screen readers keep it) or `hidden` (for text the control's own label already says).
 */
export function fadeLabel(fold: Fold, gone: 'sr-only' | 'hidden' = 'sr-only'): string {
  return cn(
    'transition-opacity',
    fold.collapsed ? 'opacity-0 duration-150' : cn('opacity-100 duration-300', !fold.reduced && 'delay-150'),
    fold.hidden && gone,
  );
}

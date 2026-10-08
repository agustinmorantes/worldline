// The tour's open state, shared by the overlay (`TourHost`), the title bar's `?` menu, the empty
// library and the contextual hints.
import { createContext, use } from 'react';
import type { TourStep } from './tour';

export interface TourControl {
  /** The tour is on screen. */
  open: boolean;
  /** Opens the tour at `step`, growing it out of the `?`. */
  openAt: (step: TourStep) => void;
  /** Set for a few seconds after the tour closes: the `?` lights up with a tip. */
  tip: boolean;
  dismissTip: () => void;
}

export const TourContext = createContext<TourControl>({
  open: false,
  openAt: () => undefined,
  tip: false,
  dismissTip: () => undefined,
});
TourContext.displayName = 'Tour';

export function useTour(): TourControl {
  return use(TourContext);
}

/** Marks the title bar's `?`, which the tour shrinks into and grows out of. */
export const HELP_ANCHOR = 'data-help-anchor';

// The source of every UI string. `es.ts` must match this shape: strings with
// parameters are functions, plurals go through `plural()`.
import { plural } from './plural';

export const en = {
  common: {
    checkpoints: (count: number): string => plural('en', count, { one: '# checkpoint', other: '# checkpoints' }),
  },
  errors: {
    loadFailed: 'Couldn’t load this. Try again in a moment.',
    actionFailed: 'That didn’t work. Try again.',
    detectionFailed: 'Couldn’t check for new saves.',
    pruningFailed: 'Automatic history cleanup failed.',
    copyDetails: 'Copy details',
  },
  notices: {
    oldDataMoved: (path: string): string =>
      `Data from a pre-release version was moved to ${path} and the app started fresh.`,
  },
};

export type Messages = typeof en;

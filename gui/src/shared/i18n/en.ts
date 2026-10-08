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
  shell: {
    library: 'Library',
    addGames: 'Add games',
    settings: 'Settings',
    inCloudBadge: (count: number): string =>
      plural('en', count, { one: '# game in your cloud isn’t on this PC yet', other: '# games in your cloud aren’t on this PC yet' }),
    help: 'Help',
    replayTour: 'Replay the tour',
    minimize: 'Minimize',
    maximize: 'Maximize',
    close: 'Close',
  },
  gameStatus: {
    newFrom: (device: string): string => `New from ${device}`,
    playedToo: (device: string): string => `${device} played too`,
    notInCloud: 'Not in the cloud yet',
    noSaves: 'No saves yet',
  },
  sync: {
    thisPc: 'This PC',
    cloud: 'Cloud',
    syncing: 'Syncing…',
    synced: (when: string): string => `Synced ${when}`,
    notSynced: 'Not synced yet',
    unreachable: 'Couldn’t reach the cloud',
    syncNow: 'Sync now',
    failed: 'Couldn’t sync.',
    turnOn: 'Turn on cloud',
    turnOnHint: 'Your saves on every device',
  },
  notices: {
    oldDataMoved: (path: string): string =>
      `Data from a pre-release version was moved to ${path} and the app started fresh.`,
  },
};

export type Messages = typeof en;

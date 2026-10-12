import { describe, expect, it } from 'vitest';
import { makeGame, makeVersion, naive } from '@/ipc/builders';
import type { GameSyncStatus } from '@/ipc/bindings/GameSyncStatus';
import { gameStatus, initials, lastPlayed, matchesTitle, sortLibrary, withRunning } from './library';

const played = (id: number, title: string, iso: string | null) =>
  makeGame({
    id,
    title,
    versions: iso === null ? [] : [makeVersion({ id, created_at: naive(new Date(iso)) })],
    last_played_at: iso === null ? null : naive(new Date(iso)),
  });

const status = (relation: GameSyncStatus['relation'], device: string | null = 'Steam Deck'): GameSyncStatus => ({
  game_uuid: 'g',
  relation,
  remote_device: device,
  remote_version_uuid: null,
  remote_version_id: null,
  auto_applied: false,
});

describe('initials', () => {
  it('takes the first letter of up to two words', () => {
    expect(initials('Elden Ring')).toBe('ER');
    expect(initials('hades')).toBe('H');
    expect(initials('The Legend of Zelda')).toBe('TL');
    expect(initials('  Sea  of Stars ')).toBe('SO');
  });
});

describe('sortLibrary', () => {
  it('puts the most recently played first and unplayed games last by title', () => {
    const games = [
      played(1, 'Zelda', null),
      played(2, 'Hades', '2026-01-01T10:00:00Z'),
      played(3, 'Celeste', null),
      played(4, 'Balatro', '2026-01-02T10:00:00Z'),
    ];
    expect(sortLibrary(games).map((g) => g.title)).toEqual(['Balatro', 'Hades', 'Celeste', 'Zelda']);
  });

  it('orders never-played games by title whatever order they arrive in', () => {
    const titles = ['Outer Wilds', 'Celeste', 'Tunic', 'Animal Well'];
    const games = titles.map((title, i) => played(i, title, null));
    const sorted = ['Animal Well', 'Celeste', 'Outer Wilds', 'Tunic'];
    expect(sortLibrary(games).map((g) => g.title)).toEqual(sorted);
    expect(sortLibrary([...games].reverse()).map((g) => g.title)).toEqual(sorted);
  });

  it('orders by activity when a newly imported checkpoint has a newer creation date', () => {
    const importedToday = makeGame({
      id: 1,
      title: 'Imported',
      versions: [makeVersion({ id: 1, created_at: '2026-10-12T12:00:00' })],
      last_played_at: '2026-02-01T10:00:00',
    });
    const playedRecently = makeGame({
      id: 2,
      title: 'Played',
      versions: [makeVersion({ id: 2, created_at: '2026-03-01T10:00:00' })],
      last_played_at: '2026-10-01T10:00:00',
    });
    expect(sortLibrary([importedToday, playedRecently]).map((game) => game.title)).toEqual(['Played', 'Imported']);
  });

  it('leaves activity unknown when checkpoints have no activity timestamp', () => {
    const unknown = makeGame({ id: 1, title: 'Unknown', versions: [makeVersion({ id: 1 })], last_played_at: null });
    expect(lastPlayed(unknown)).toBeNull();
    expect(gameStatus(unknown, { on: false, reported: false, status: undefined })).toEqual({ kind: 'saved' });
  });

  it('uses game-wide activity even when the active slot version is older', () => {
    const game = makeGame({
      id: 1,
      title: 'Two slots',
      versions: [makeVersion({ id: 1, created_at: '2026-01-01T00:00:00' })],
      last_played_at: '2026-07-01T00:00:00',
    });
    expect(lastPlayed(game)).toEqual(new Date('2026-07-01T00:00:00Z'));
  });

  it('lets mock games infer activity from checkpoint dates unless explicitly overridden', () => {
    const versions = [
      makeVersion({ id: 1, created_at: '2026-01-01T00:00:00' }),
      makeVersion({ id: 2, created_at: '2026-02-01T00:00:00' }),
    ];
    expect(lastPlayed(makeGame({ id: 1, title: 'Derived', versions })))
      .toEqual(new Date('2026-02-01T00:00:00Z'));
    expect(lastPlayed(makeGame({ id: 2, title: 'Explicit unknown', versions, last_played_at: null }))).toBeNull();
  });
});

describe('matchesTitle', () => {
  it('matches anywhere in the title, ignoring case', () => {
    expect(matchesTitle('Elden Ring', 'ring')).toBe(true);
    expect(matchesTitle('Elden Ring', 'ELD')).toBe(true);
    expect(matchesTitle('Elden Ring', 'hades')).toBe(false);
  });

  it('ignores accents on either side', () => {
    expect(matchesTitle('Pokémon Legends', 'pokemon')).toBe(true);
    expect(matchesTitle('Celeste', 'céleste')).toBe(true);
    expect(matchesTitle('Ōkami', 'okami')).toBe(true);
  });

  it('treats a blank query as matching everything', () => {
    expect(matchesTitle('Hades', '')).toBe(true);
    expect(matchesTitle('Hades', '   ')).toBe(true);
  });
});

describe('gameStatus', () => {
  const game = played(1, 'Hades', '2026-01-01T10:00:00Z');
  const off = { on: false, reported: false, status: undefined };

  it('says when it was last played', () => {
    expect(gameStatus(game, off)).toEqual({ kind: 'last-played', at: new Date('2026-01-01T10:00:00Z') });
  });

  it('says when there is nothing yet', () => {
    expect(gameStatus(played(2, 'Celeste', null), off)).toEqual({ kind: 'no-saves' });
  });

  it('names the device whose progress is ahead or split off', () => {
    expect(gameStatus(game, { on: true, reported: true, status: status('fast_forward') }))
      .toEqual({ kind: 'new-from', device: 'Steam Deck' });
    expect(gameStatus(game, { on: true, reported: true, status: status('diverged') }))
      .toEqual({ kind: 'played-too', device: 'Steam Deck' });
  });

  it('flags a game the cloud has not seen, once a sync has reported', () => {
    expect(gameStatus(game, { on: true, reported: true, status: undefined })).toEqual({ kind: 'not-in-cloud' });
    expect(gameStatus(game, { on: true, reported: false, status: undefined }).kind).toBe('last-played');
  });

  it('treats an in-sync game as last played', () => {
    expect(gameStatus(game, { on: true, reported: true, status: status('in_sync', null) }).kind).toBe('last-played');
  });
});

describe('running games', () => {
  it('puts recording above every other status', () => {
    const game = played(1, 'Hades', '2026-01-01T10:00:00Z');
    expect(gameStatus(game, { on: true, reported: true, status: status('diverged') }, true)).toEqual({ kind: 'recording' });
  });

  it('adds a started game once and drops a stopped one', () => {
    expect(withRunning(undefined, 1, true)).toEqual([1]);
    expect(withRunning([1, 2], 1, true)).toEqual([2, 1]);
    expect(withRunning([1, 2], 1, false)).toEqual([2]);
    expect(withRunning([2], 1, false)).toEqual([2]);
  });
});

import { describe, expect, it } from 'vitest';
import { makeGame, makeVersion, naive } from '@/ipc/builders';
import type { GameSyncStatus } from '@/ipc/bindings/GameSyncStatus';
import { gameStatus, initials, sortLibrary, withRunning } from './library';

const played = (id: number, title: string, iso: string | null) =>
  makeGame({ id, title, versions: iso === null ? [] : [makeVersion({ id, created_at: naive(new Date(iso)) })] });

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

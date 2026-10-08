import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { isFolderUnreachable, setSyncOutcome, useFolderUnreachable } from './sync-activity';

const unreachable = { message: 'Sync folder is unavailable: os error 3', kind: 'sync_folder_unreachable' };

describe('isFolderUnreachable', () => {
  it('tells the unreachable kind from other failures', () => {
    expect(isFolderUnreachable(unreachable)).toBe(true);
    expect(isFolderUnreachable({ message: 'disk full', kind: null })).toBe(false);
    expect(isFolderUnreachable('Sync folder is unavailable')).toBe(false);
    expect(isFolderUnreachable(null)).toBe(false);
  });
});

describe('useFolderUnreachable', () => {
  it('follows the latest sync: set by an unreachable folder, cleared by a success or another failure', () => {
    const { result } = renderHook(() => useFolderUnreachable());
    expect(result.current).toBe(false);

    act(() => { setSyncOutcome(unreachable); });
    expect(result.current).toBe(true);
    act(() => { setSyncOutcome(null); });
    expect(result.current).toBe(false);

    act(() => { setSyncOutcome(unreachable); });
    act(() => { setSyncOutcome({ message: 'disk full', kind: null }); });
    expect(result.current).toBe(false);
  });
});

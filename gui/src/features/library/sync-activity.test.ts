import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { setSyncOutcome, syncBlock, useSyncBlock } from './sync-activity';

const unreachable = { message: 'Sync folder is unavailable: os error 3', kind: 'sync_folder_unreachable' };
const newer = { message: 'needs a newer version', kind: 'update_required' };

describe('syncBlock', () => {
  it('tells the blocking kinds from other failures', () => {
    expect(syncBlock(unreachable)).toBe('unreachable');
    expect(syncBlock(newer)).toBe('update');
    expect(syncBlock({ message: 'disk full', kind: null })).toBeNull();
    expect(syncBlock('Sync folder is unavailable')).toBeNull();
    expect(syncBlock(null)).toBeNull();
  });
});

describe('useSyncBlock', () => {
  it('follows the latest sync: set by a blocking failure, cleared by a success or another failure', () => {
    const { result } = renderHook(() => useSyncBlock());
    expect(result.current).toBeNull();

    act(() => { setSyncOutcome(unreachable); });
    expect(result.current).toBe('unreachable');
    act(() => { setSyncOutcome(newer); });
    expect(result.current).toBe('update');
    act(() => { setSyncOutcome(null); });
    expect(result.current).toBeNull();

    act(() => { setSyncOutcome(unreachable); });
    act(() => { setSyncOutcome({ message: 'disk full', kind: null }); });
    expect(result.current).toBeNull();
  });
});

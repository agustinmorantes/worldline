import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ toastError: vi.fn(), logError: vi.fn() }));
vi.mock('sonner', () => ({ toast: { error: mocks.toastError } }));
vi.mock('@tauri-apps/plugin-log', () => ({ error: mocks.logError }));

import { errorDetails, reportError } from './report';

describe('reportError', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.logError.mockResolvedValue(undefined);
  });

  it('shows the given message, never the raw error, and logs the raw error', () => {
    reportError('Couldn’t restore', 'IO error: access denied (os error 5)');

    expect(mocks.toastError).toHaveBeenCalledWith('Couldn’t restore', expect.objectContaining({
      action: expect.objectContaining({ label: 'Copy details' }) as unknown,
    }));
    expect(mocks.logError).toHaveBeenCalledWith('Couldn’t restore: IO error: access denied (os error 5)');
  });

  it('copies the raw error from the toast action', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });

    reportError('Couldn’t sync', 'remote folder missing');
    const [, options] = mocks.toastError.mock.calls[0] as [string, { action: { onClick: () => void } }];
    options.action.onClick();

    await vi.waitFor(() => { expect(writeText).toHaveBeenCalledWith('remote folder missing'); });
  });
});

describe('errorDetails', () => {
  it('keeps strings and describes errors and objects', () => {
    expect(errorDetails('plain')).toBe('plain');
    expect(errorDetails(new TypeError('bad'))).toMatch(/^TypeError: bad/);
    expect(errorDetails({ code: 5 })).toBe('{"code":5}');
  });
});

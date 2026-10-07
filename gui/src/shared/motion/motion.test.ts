import { describe, expect, it } from 'vitest';
import { reducedMotionConfig } from './motion';

describe('reducedMotionConfig', () => {
  it('maps the preference to motion’s MotionConfig setting', () => {
    expect(reducedMotionConfig('system')).toBe('user');
    expect(reducedMotionConfig('full')).toBe('never');
    expect(reducedMotionConfig('reduced')).toBe('always');
  });
});

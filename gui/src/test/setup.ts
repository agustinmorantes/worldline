import '@testing-library/jest-dom';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';
import * as React from 'react';

// Automatically cleanup after each test
afterEach(() => {
  cleanup();
});

// Tauri APIs: components reach the backend through `@/ipc/commands`, which
// tests mock per file; anything left unmocked fails loudly.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://${path}`,
}));

// jsdom has no media queries: the window is always wide.
window.matchMedia = (query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener: () => undefined,
  removeEventListener: () => undefined,
  addListener: () => undefined,
  removeListener: () => undefined,
  dispatchEvent: () => false,
});

vi.mock('@tauri-apps/plugin-log', () => ({
  error: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('motion/react', () => ({
  motion: new Proxy({}, {
    get: (_target, tag: string) =>
      ({ children, layoutId: _layoutId, ...props }: React.HTMLAttributes<HTMLElement> & { layoutId?: string }) =>
        React.createElement(tag, props, children),
  }),
  AnimatePresence: ({ children }: { children: React.ReactNode }) => children,
  MotionConfig: ({ children }: { children: React.ReactNode }) => children,
  useReducedMotion: () => false,
  useIsPresent: () => true,
  useAnimationControls: () => ({
    set: vi.fn(),
    start: vi.fn().mockResolvedValue(undefined),
  }),
}));

// jsdom has no ResizeObserver; Radix measures popovers and tooltips with it.
globalThis.ResizeObserver = class {
  observe(): void { /* nothing to measure */ }
  unobserve(): void { /* nothing to measure */ }
  disconnect(): void { /* nothing to measure */ }
};

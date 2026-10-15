import { fireEvent, render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { GameIcon } from './GameIcon';

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

describe('GameIcon', () => {
  it('falls back after a failed image and retries a new path or revision', () => {
    const { container, rerender } = render(<GameIcon title="Elden Ring" iconPath="/missing.png" />);
    fireEvent.error(screen.getByAltText(''));
    expect(screen.getByText('ER')).toBeInTheDocument();
    expect(container.querySelector('img')).toBeNull();
    rerender(<GameIcon title="Elden Ring" iconPath="/missing.png" />);
    expect(container.querySelector('img')).toBeNull();
    rerender(<GameIcon title="Elden Ring" iconPath="/valid.png" iconRevision="aa" />);
    expect(container.querySelector('img')).not.toBeNull();
    fireEvent.error(screen.getByAltText(''));
    rerender(<GameIcon title="Elden Ring" iconPath="/valid.png" iconRevision="bb" />);
    expect(container.querySelector('img')?.getAttribute('src')).toBe('asset:///valid.png?v=bb');
  });

  it('shows the initials on the accent gradient', () => {
    const { container } = render(<GameIcon title="Elden Ring" hue={80} />);
    expect(screen.getByText('ER')).toBeInTheDocument();
    expect((container.firstChild as HTMLElement).style.background).toContain('oklch(0.8 0.13 80)');
  });

  it('applies the given size', () => {
    const { container } = render(<GameIcon title="Test" size={48} />);
    const el = container.firstChild as HTMLElement;
    expect(el.style.width).toBe('48px');
    expect(el.style.height).toBe('48px');
  });

  it('renders the icon file when there is one', () => {
    render(<GameIcon title="Test" iconPath="/mock/icons/1.png" />);
    expect(document.querySelector('img')?.getAttribute('src')).toBe('asset:///mock/icons/1.png');
  });

  it('shows an icon that arrives while the initials are shown', () => {
    const { rerender } = render(<GameIcon title="Elden Ring" />);
    expect(screen.getByText('ER')).toBeInTheDocument();
    rerender(<GameIcon title="Elden Ring" iconPath="/mock/icons/1.png" />);
    expect(screen.queryByText('ER')).not.toBeInTheDocument();
    expect(document.querySelector('img')?.getAttribute('src')).toBe('asset:///mock/icons/1.png');
  });

  it('reloads an icon replaced at the same path, and only then', () => {
    const { rerender } = render(<GameIcon title="Test" iconPath="/mock/icons/1.png" iconRevision="sha256:aa" />);
    const src = (): string | null | undefined => document.querySelector('img')?.getAttribute('src');
    expect(src()).toBe('asset:///mock/icons/1.png?v=sha256%3Aaa');
    rerender(<GameIcon title="Test" iconPath="/mock/icons/1.png" iconRevision="sha256:aa" />);
    expect(src()).toBe('asset:///mock/icons/1.png?v=sha256%3Aaa');
    rerender(<GameIcon title="Test" iconPath="/mock/icons/1.png" iconRevision="sha256:bb" />);
    expect(src()).toBe('asset:///mock/icons/1.png?v=sha256%3Abb');
    // A replaced icon swaps in place; only one arriving from the initials settles in.
    expect(document.querySelector('img')?.hasAttribute('initial')).toBe(false);
  });
});

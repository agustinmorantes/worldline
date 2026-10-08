import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { GameIcon } from './GameIcon';

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

describe('GameIcon', () => {
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
});

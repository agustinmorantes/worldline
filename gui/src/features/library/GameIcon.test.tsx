import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { GameIcon } from './GameIcon';

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));


describe('GameIcon', () => {
  it('renders the first letter of the title', () => {
    render(<GameIcon title="Elden Ring" />);
    expect(screen.getByText('E')).toBeInTheDocument();
  });

  it('uppercases the first letter', () => {
    render(<GameIcon title="dark souls" />);
    expect(screen.getByText('D')).toBeInTheDocument();
  });

  it('generates a consistent color for the same title', () => {
    const { container: c1 } = render(<GameIcon title="Hollow Knight" />);
    const { container: c2 } = render(<GameIcon title="Hollow Knight" />);
    const el1 = c1.firstChild as HTMLElement;
    const el2 = c2.firstChild as HTMLElement;
    expect(el1.style.backgroundColor).toEqual(el2.style.backgroundColor);
  });

  it('generates different colors for different titles', () => {
    const { container: c1 } = render(<GameIcon title="Elden Ring" />);
    const { container: c2 } = render(<GameIcon title="Dark Souls" />);
    const el1 = c1.firstChild as HTMLElement;
    const el2 = c2.firstChild as HTMLElement;
    expect(el1.style.backgroundColor).not.toEqual(el2.style.backgroundColor);
  });

  it('applies the given size', () => {
    const { container } = render(<GameIcon title="Test" size={48} />);
    const el = container.firstChild as HTMLElement;
    expect(el.style.width).toBe('48px');
    expect(el.style.height).toBe('48px');
  });

  it('renders an img when iconPath is provided', () => {
    render(<GameIcon title="Test" iconPath="/mock/icons/1.png" />);
    const img = document.querySelector('img');
    expect(img).not.toBeNull();
    expect(img?.alt).toBe('Test');
  });
});

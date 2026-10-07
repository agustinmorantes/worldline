import { describe, expect, it } from 'vitest';
import { en, type Messages } from './en';
import { es } from './es';
import { plural } from './plural';
import { resolveLocale } from './locale';
import { dateFnsLocale, formatRelative } from './dates';

/** Every key path of a messages object, functions included. */
function keyPaths(messages: object, prefix = ''): string[] {
  return Object.entries(messages).flatMap(([key, value]) =>
    typeof value === 'object' && value !== null
      ? keyPaths(value as object, `${prefix}${key}.`)
      : [`${prefix}${key}`],
  );
}

describe('messages', () => {
  it('has every English key in Spanish', () => {
    // `es` is typed `Messages`, so a missing key fails `tsc`; this guards the runtime shape.
    const spanish: Messages = es;
    expect(keyPaths(spanish).sort()).toEqual(keyPaths(en).sort());
  });

  it('renders plural forms in both languages', () => {
    expect(en.common.checkpoints(1)).toBe('1 checkpoint');
    expect(en.common.checkpoints(3)).toBe('3 checkpoints');
    expect(en.common.checkpoints(1200)).toBe('1,200 checkpoints');
    expect(es.common.checkpoints(1)).toBe('1 punto de control');
    expect(es.common.checkpoints(0)).toBe('0 puntos de control');
  });
});

describe('plural', () => {
  it('picks the CLDR category and formats the count', () => {
    expect(plural('en', 1, { one: '# file', other: '# files' })).toBe('1 file');
    expect(plural('en', 2, { one: '# file', other: '# files' })).toBe('2 files');
    expect(plural('es', 1, { one: '# archivo', other: '# archivos' })).toBe('1 archivo');
    expect(plural('es', 1.5, { one: '# hora', other: '# horas' })).toBe('1,5 horas');
  });

  it('falls back to other when a category is missing', () => {
    expect(plural('en', 0, { one: '# file', other: '# files' })).toBe('0 files');
  });
});

describe('resolveLocale', () => {
  it('uses an explicit choice', () => {
    expect(resolveLocale('es', ['en-US'])).toBe('es');
  });

  it('follows the system language, falling back to English', () => {
    expect(resolveLocale('system', ['es-AR', 'en'])).toBe('es');
    expect(resolveLocale('system', ['fr-FR', 'es'])).toBe('es');
    expect(resolveLocale('system', ['fr-FR'])).toBe('en');
    expect(resolveLocale('system', [])).toBe('en');
  });
});

describe('dates', () => {
  const now = new Date('2026-10-07T12:00:00Z');

  it('formats relative times per locale', () => {
    expect(formatRelative(new Date('2026-10-07T11:55:00Z'), 'en', now)).toBe('5 minutes ago');
    expect(formatRelative(new Date('2026-10-06T12:00:00Z'), 'en', now)).toBe('yesterday');
    expect(formatRelative(new Date('2026-10-07T09:00:00Z'), 'es', now)).toBe('hace 3 horas');
  });

  it('maps locales to date-fns locales', () => {
    expect(dateFnsLocale('en').code).toBe('en-US');
    expect(dateFnsLocale('es').code).toBe('es');
  });
});

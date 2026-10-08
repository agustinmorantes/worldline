import type { Locale as DateFnsLocale } from 'date-fns';
import { enUS, es } from 'date-fns/locale';
import { en } from './en';
import { es as esMessages } from './es';
import type { Locale } from './locale';

const DATE_FNS_LOCALES: Record<Locale, DateFnsLocale> = { en: enUS, es };

/** For date-fns helpers that take a `locale` option. */
export function dateFnsLocale(locale: Locale): DateFnsLocale {
  return DATE_FNS_LOCALES[locale];
}

export function formatDate(date: Date, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(date);
}

export function formatTime(date: Date, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { timeStyle: 'short' }).format(date);
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 60 * 60],
  ['month', 30.44 * 24 * 60 * 60],
  ['week', 7 * 24 * 60 * 60],
  ['day', 24 * 60 * 60],
  ['hour', 60 * 60],
];

const JUST_NOW: Record<Locale, string> = { en: en.common.justNow, es: esMessages.common.justNow };

/** "5 minutes ago", "yesterday", "hace 3 horas". A future date (a stale `now`) reads as now; under a minute is "just now". */
export function formatRelative(date: Date, locale: Locale, now = new Date()): string {
  const seconds = Math.min(0, (date.getTime() - now.getTime()) / 1000);
  if (seconds > -60) return JUST_NOW[locale];
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return format.format(Math.trunc(seconds / size), unit);
  }
  return format.format(Math.trunc(seconds / 60), 'minute');
}

import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs))
}

export function timeAgo(date: Date, locale = 'en'): string {
  const diff = new Date().getTime() - date.getTime();
  const seconds = Math.floor(diff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  const months = Math.floor(days / 30.44); // average days in a month
  const years = Math.floor(months / 12);

  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });

  if (Math.abs(years) > 0) return rtf.format(-years, 'year');
  if (Math.abs(months) > 0) return rtf.format(-months, 'month');
  if (Math.abs(days) > 0) return rtf.format(-days, 'day');
  if (Math.abs(hours) > 0) return rtf.format(-hours, 'hour');
  if (Math.abs(minutes) > 0) return rtf.format(-minutes, 'minute');
  return rtf.format(-seconds, 'second');
}

export function mockIconPath(title: string): string {
  return title.split(' ').join('_').toLowerCase() + ".ico";
}

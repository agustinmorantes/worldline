import { getPrefs, usePrefs } from '@/shared/prefs/prefs';
import { en, type Messages } from './en';
import { es } from './es';
import { resolveLocale, type LanguagePref, type Locale } from './locale';

export type { Messages } from './en';
export type { Locale, LanguagePref } from './locale';
export { LOCALES, resolveLocale } from './locale';
export { plural } from './plural';
export { dateFnsLocale, formatDate, formatRelative, formatTime } from './dates';

const MESSAGES: Record<Locale, Messages> = { en, es };

function localeOf(language: LanguagePref): Locale {
  return resolveLocale(language, navigator.languages);
}

export function useLocale(): Locale {
  return localeOf(usePrefs().language);
}

export function useT(): Messages {
  return MESSAGES[useLocale()];
}

/** For code outside React (query callbacks, event handlers). */
export function getT(): Messages {
  return MESSAGES[localeOf(getPrefs().language)];
}

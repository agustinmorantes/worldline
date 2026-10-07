export const LOCALES = ['en', 'es'] as const;
export type Locale = (typeof LOCALES)[number];
/** The language preference: a locale, or follow the system. */
export type LanguagePref = 'system' | Locale;

/** `system` picks the first supported system language, falling back to English. */
export function resolveLocale(pref: LanguagePref, systemLanguages: readonly string[]): Locale {
  if (pref !== 'system') return pref;
  for (const tag of systemLanguages) {
    const language = tag.split('-')[0]?.toLowerCase();
    const match = LOCALES.find((locale) => locale === language);
    if (match !== undefined) return match;
  }
  return 'en';
}

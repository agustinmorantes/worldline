import type { Locale } from './locale';

/** One string per CLDR plural category; `#` is replaced by the formatted count. */
export type PluralForms = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };

export function plural(locale: Locale, count: number, forms: PluralForms): string {
  const category = new Intl.PluralRules(locale).select(count);
  const form = forms[category] ?? forms.other;
  return form.replace(/#/g, new Intl.NumberFormat(locale).format(count));
}

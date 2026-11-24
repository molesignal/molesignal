/**
 * The languages the interface is offered in, and how each one is named to the
 * browser, to `Intl`, to the docs site and to the person choosing it.
 *
 * A locale id is lower case (`en-us`, `pt-br`): it is what the preference
 * store, the backend and the translation directories use. Nothing here loads
 * translations, so build scripts can import it too.
 */

export const SUPPORTED_LOCALES = [
  'en-us',
  'zh-cn',
  'zh-tw',
  'ja-jp',
  'ko-kr',
  'de',
  'es',
  'fr',
  'pt-br',
] as const;

export type Locale = (typeof SUPPORTED_LOCALES)[number];

export const DEFAULT_LOCALE: Locale = 'en-us';

/** Each language in its own words, as a language picker lists it. */
export const LOCALE_LABELS: Record<Locale, string> = {
  'en-us': 'English',
  'zh-cn': '简体中文',
  'zh-tw': '繁體中文',
  'ja-jp': '日本語',
  'ko-kr': '한국어',
  de: 'Deutsch',
  es: 'Español',
  fr: 'Français',
  'pt-br': 'Português (Brasil)',
};

/** The BCP 47 tag `Intl`, `<html lang>` and `lang` attributes take. */
const INTL_TAGS: Record<Locale, string> = {
  'en-us': 'en-US',
  'zh-cn': 'zh-CN',
  'zh-tw': 'zh-TW',
  'ja-jp': 'ja-JP',
  'ko-kr': 'ko-KR',
  de: 'de-DE',
  es: 'es-ES',
  fr: 'fr-FR',
  'pt-br': 'pt-BR',
};

export function isLocale(value: string | null | undefined): value is Locale {
  return (SUPPORTED_LOCALES as readonly string[]).includes(value ?? '');
}

/**
 * The supported locale a language tag asks for, such as `zh-Hant-HK` or
 * `pt-PT`; none when the product has no translation in that language.
 */
export function matchLocale(tag: string | null | undefined): Locale | undefined {
  const lower = tag?.trim().toLowerCase().replace(/_/g, '-');

  if (!lower) return undefined;
  if (isLocale(lower)) return lower;

  const [language, ...rest] = lower.split('-');

  if (language === 'zh') {
    const traditional = rest.some((part) => ['hant', 'tw', 'hk', 'mo'].includes(part));
    return traditional ? 'zh-tw' : 'zh-cn';
  }

  switch (language) {
    case 'en':
      return 'en-us';
    case 'ja':
      return 'ja-jp';
    case 'ko':
      return 'ko-kr';
    case 'de':
    case 'es':
    case 'fr':
      return language;
    case 'pt':
      return 'pt-br';
    default:
      return undefined;
  }
}

/** The supported locale for a language id, falling back to English. */
export function toLocale(language: string | null | undefined): Locale {
  return matchLocale(language) ?? DEFAULT_LOCALE;
}

/** The BCP 47 tag for `Intl` formatters and `lang` attributes. */
export function intlLocale(language: string | null | undefined): string {
  return INTL_TAGS[toLocale(language)];
}

/** The directory of docs.molesignal.com that holds each language's pages. */
const DOCS_DIRECTORIES = {
  'en-us': 'en-US',
  'zh-cn': 'zh-Hans',
  'zh-tw': 'zh-Hant',
  'ja-jp': 'ja',
  'ko-kr': 'ko',
  de: 'de',
  es: 'es',
  fr: 'fr',
  'pt-br': 'pt-BR',
} as const satisfies Record<Locale, string>;

export type DocsLanguage = (typeof DOCS_DIRECTORIES)[Locale];

/** The language of docs.molesignal.com pages to link: the site has every language the product has. */
export function docsLocale(language: string | null | undefined): DocsLanguage {
  return DOCS_DIRECTORIES[toLocale(language)];
}

/** Whether the reader is in mainland China, where domestic vendors come first in a list. */
export function isMainlandChinese(language: string | null | undefined): boolean {
  return toLocale(language) === 'zh-cn';
}

/** Whether the language is written in Chinese characters without spaces between words. */
export function isChinese(language: string | null | undefined): boolean {
  return toLocale(language).startsWith('zh-');
}

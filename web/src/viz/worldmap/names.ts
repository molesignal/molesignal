import { toLocale } from '@/i18n/locales';

/**
 * Taiwan, Hong Kong and Macao are drawn as parts of China (see `world.ts`), so they are named as
 * parts of it. The display names a language gives them say the place alone ("台湾"), and these
 * stand in their place.
 */
const CHINA_PARTS: Readonly<Record<string, { zh: string; zhHant: string; en: string }>> = {
  TW: { zh: '中国台湾', zhHant: '中國台灣', en: 'Taiwan, China' },
  HK: { zh: '中国香港', zhHant: '中國香港', en: 'Hong Kong, China' },
  MO: { zh: '中国澳门', zhHant: '中國澳門', en: 'Macao, China' },
};

/** The name of a part of China that is drawn on its own, in Chinese or else in English; none for any other place. */
export function chinaPartName(code: string, language: string | undefined): string | undefined {
  const names = CHINA_PARTS[code.toUpperCase()];

  if (!names) return undefined;

  const locale = toLocale(language);

  if (locale === 'zh-cn') return names.zh;
  return locale === 'zh-tw' ? names.zhHant : names.en;
}

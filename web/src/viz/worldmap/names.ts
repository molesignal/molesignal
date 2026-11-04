/**
 * Taiwan, Hong Kong and Macao are drawn as parts of China (see `world.ts`), so they are named as
 * parts of it. The display names a language gives them say the place alone ("台湾"), and these
 * stand in their place.
 */
const CHINA_PARTS: Readonly<Record<string, { zh: string; en: string }>> = {
  TW: { zh: '中国台湾', en: 'Taiwan, China' },
  HK: { zh: '中国香港', en: 'Hong Kong, China' },
  MO: { zh: '中国澳门', en: 'Macao, China' },
};

/** The name of a part of China that is drawn on its own, in Chinese or else in English; none for any other place. */
export function chinaPartName(code: string, language: string | undefined): string | undefined {
  const names = CHINA_PARTS[code.toUpperCase()];

  if (!names) return undefined;

  return language?.toLowerCase().startsWith('zh') ? names.zh : names.en;
}

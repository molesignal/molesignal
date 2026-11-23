import { docsLocale } from '@/i18n/locales';

export type RumDocumentationSection =
  | 'explorer'
  | 'sdk'
  | 'source-maps'
  | 'sampling'
  | 'privacy'
  | 'session-replay';

const DOCUMENTATION_PATHS: Record<RumDocumentationSection, string> = {
  explorer: 'explorer',
  sdk: 'browser-sdk',
  'source-maps': 'source-maps',
  sampling: 'sampling',
  privacy: 'privacy',
  'session-replay': 'session-replay',
};

export function rumDocumentationLocale(language?: string): 'en-US' | 'zh-Hans' {
  return docsLocale(language);
}

export function rumDocumentationUrl(
  language: string | undefined,
  section: RumDocumentationSection,
): string {
  const locale = rumDocumentationLocale(language);
  return `https://docs.molesignal.com/${locale}/rum/${DOCUMENTATION_PATHS[section]}`;
}

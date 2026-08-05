export type RumDocumentationSection =
  | 'sdk'
  | 'source-maps'
  | 'sampling'
  | 'privacy'
  | 'session-replay';

const DOCUMENTATION_PATHS: Record<RumDocumentationSection, string> = {
  sdk: 'browser-sdk',
  'source-maps': 'source-maps',
  sampling: 'sampling',
  privacy: 'privacy',
  'session-replay': 'session-replay',
};

export function rumDocumentationLocale(language?: string): 'en-US' | 'zh-Hans' {
  return language?.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';
}

export function rumDocumentationUrl(
  language: string | undefined,
  section: RumDocumentationSection,
): string {
  const locale = rumDocumentationLocale(language);
  return `https://docs.molesignal.io/${locale}/rum/${DOCUMENTATION_PATHS[section]}`;
}

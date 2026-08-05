import { describe, expect, it } from 'vitest';

import { rumDocumentationLocale, rumDocumentationUrl } from './documentation';

describe('RUM documentation links', () => {
  it.each([
    ['zh-CN', 'zh-Hans'],
    ['zh-Hans', 'zh-Hans'],
    ['en-US', 'en-US'],
    [undefined, 'en-US'],
  ] as const)('maps %s to the matching documentation locale', (language, locale) => {
    expect(rumDocumentationLocale(language)).toBe(locale);
  });

  it.each([
    ['sdk', 'browser-sdk'],
    ['source-maps', 'source-maps'],
    ['sampling', 'sampling'],
    ['privacy', 'privacy'],
    ['session-replay', 'session-replay'],
  ] as const)('routes %s to its Chinese documentation page', (section, path) => {
    expect(rumDocumentationUrl('zh-CN', section)).toBe(
      `https://docs.molesignal.io/zh-Hans/rum/${path}`,
    );
  });
});

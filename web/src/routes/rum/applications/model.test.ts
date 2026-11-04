import { describe, expect, it } from 'vitest';

import type { RetentionFilter, RumApplication } from '@/api/rum';

import {
  applicationIdFromName,
  byName,
  DEFAULT_SETTINGS,
  guideSourceOf,
  isApplicationId,
  MAX_ISSUES_PER_HOUR,
  MAX_SESSION_SAMPLE_RATE,
  sameSettings,
  validSettings,
  withScrub,
} from './model';

describe('applicationIdFromName', () => {
  it('makes lower-case words joined by single dashes, as the server does', () => {
    expect(applicationIdFromName('Checkout Web')).toBe('checkout-web');
    expect(applicationIdFromName('  --Shop / iOS (beta)  ')).toBe('shop-ios-beta');
    expect(applicationIdFromName('日本語')).toBe('app');
    expect(applicationIdFromName('')).toBe('app');
  });

  it('is short enough, and does not end in a dash', () => {
    const id = applicationIdFromName(`${'a'.repeat(63)} tail`);

    expect(id.length).toBeLessThanOrEqual(64);
    expect(id.endsWith('-')).toBe(false);
  });
});

describe('isApplicationId', () => {
  it('takes what SDKs can be configured with', () => {
    expect(isApplicationId('shop-web')).toBe(true);
    expect(isApplicationId('shop.web:2_a')).toBe(true);
    expect(isApplicationId('../x')).toBe(false);
    expect(isApplicationId('a'.repeat(65))).toBe(false);
    expect(isApplicationId('')).toBe(false);
  });
});

describe('settings', () => {
  it('collect everything and mask text until the reader says otherwise', () => {
    expect(DEFAULT_SETTINGS).toEqual({
      collectIp: true,
      collectGeo: true,
      defaultPrivacy: 'mask',
      issuesPerHour: 100,
      scrub: ['secrets', 'emails', 'phones', 'cards', 'national_ids', 'url_params'],
      sessionSampleRate: 100,
      retentionFilters: [],
      keepErrors: true,
      sdk: {},
      segments: [],
    });
  });

  it('are the same when every part is', () => {
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS })).toBe(true);
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, collectGeo: false })).toBe(false);
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, defaultPrivacy: 'allow' })).toBe(false);
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, issuesPerHour: 20 })).toBe(false);
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, sessionSampleRate: 50 })).toBe(false);
    expect(sameSettings(DEFAULT_SETTINGS, { ...DEFAULT_SETTINGS, scrub: ['emails'] })).toBe(false);
  });

  it('do not mind the order the kinds to scrub come in', () => {
    const shuffled = { ...DEFAULT_SETTINGS, scrub: [...DEFAULT_SETTINGS.scrub].reverse() };

    expect(sameSettings(DEFAULT_SETTINGS, shuffled)).toBe(true);
  });

  it('turn a kind to scrub on or off and keep the others in the order they are shown', () => {
    expect(withScrub(DEFAULT_SETTINGS.scrub, 'phones', false)).toEqual(['secrets', 'emails', 'cards', 'national_ids', 'url_params']);
    expect(withScrub(['url_params', 'emails'], 'secrets', true)).toEqual(['secrets', 'emails', 'url_params']);
    expect(withScrub(['emails'], 'emails', true)).toEqual(['emails']);
    expect(withScrub(['emails'], 'emails', false)).toEqual([]);
  });

  it('take the share of sessions kept the server takes: a whole number of percent from none to all', () => {
    const with_ = (sessionSampleRate: number) => ({ ...DEFAULT_SETTINGS, sessionSampleRate });

    expect(validSettings(with_(0))).toBe(true);
    expect(validSettings(with_(MAX_SESSION_SAMPLE_RATE))).toBe(true);
    expect(validSettings(with_(-1))).toBe(false);
    expect(validSettings(with_(MAX_SESSION_SAMPLE_RATE + 1))).toBe(false);
    expect(validSettings(with_(12.5))).toBe(false);
    expect(validSettings(with_(Number.NaN))).toBe(false);
  });

  it('take the issues an hour the server takes: a whole number from one to the most', () => {
    const with_ = (issuesPerHour: number) => ({ ...DEFAULT_SETTINGS, issuesPerHour });

    expect(validSettings(DEFAULT_SETTINGS)).toBe(true);
    expect(validSettings(with_(1))).toBe(true);
    expect(validSettings(with_(MAX_ISSUES_PER_HOUR))).toBe(true);
    expect(validSettings(with_(0))).toBe(false);
    expect(validSettings(with_(MAX_ISSUES_PER_HOUR + 1))).toBe(false);
    expect(validSettings(with_(2.5))).toBe(false);
    expect(validSettings(with_(Number.NaN))).toBe(false);
  });
});

describe('settings with retention filters', () => {
  const filter = (name: string, rate = 100, values = ['prod']): RetentionFilter => ({
    id: '',
    name,
    rate,
    conditions: [{ field: 'env', op: 'is', values }],
  });
  const with_ = (retentionFilters: RetentionFilter[]) => ({ ...DEFAULT_SETTINGS, retentionFilters });

  it('keep every error and have no filter until told otherwise', () => {
    expect(DEFAULT_SETTINGS.keepErrors).toBe(true);
    expect(DEFAULT_SETTINGS.retentionFilters).toEqual([]);
  });

  it('differ when a filter, its order or the errors kept differ', () => {
    const saved = with_([filter('A'), filter('B', 20, ['dev'])]);

    expect(sameSettings(saved, with_([filter('A'), filter('B', 20, ['dev'])]))).toBe(true);
    expect(sameSettings(saved, with_([filter('B', 20, ['dev']), filter('A')]))).toBe(false);
    expect(sameSettings(saved, with_([filter('A'), filter('B', 30, ['dev'])]))).toBe(false);
    expect(sameSettings(saved, with_([filter('A'), filter('B', 20, ['dev', 'staging'])]))).toBe(false);
    expect(sameSettings(saved, { ...saved, keepErrors: false })).toBe(false);
  });

  it('are taken when every filter is named apart, has a share and values to compare', () => {
    expect(validSettings(with_([filter('Production'), filter('Beta', 10, ['beta'])]))).toBe(true);
    expect(validSettings(with_([filter('Production'), filter('production')]))).toBe(false);
    expect(validSettings(with_([filter('   ')]))).toBe(false);
    expect(validSettings(with_([filter('A', 101)]))).toBe(false);
    expect(validSettings(with_([filter('A', Number.NaN)]))).toBe(false);
    expect(validSettings(with_([filter('A', 10, [])]))).toBe(false);
  });
});

describe('setup guides', () => {
  it('belong to the platforms that have one', () => {
    expect(guideSourceOf('browser')).toBe('rum');
    expect(guideSourceOf('flutter')).toBe('rum-flutter');
    expect(guideSourceOf('android')).toBe('rum-android');
    expect(guideSourceOf('ios')).toBe('rum-ios');
    expect(guideSourceOf('react-native')).toBeNull();
    expect(guideSourceOf('other')).toBeNull();
  });
});

describe('byName', () => {
  it('lists applications by name, then identifier', () => {
    const application = (id: string, name: string) => ({ id, name }) as RumApplication;

    expect(byName([application('b', 'Shop'), application('a', 'Shop'), application('c', 'Admin')]).map((a) => a.id)).toEqual([
      'c',
      'a',
      'b',
    ]);
  });
});

import { describe, expect, it } from 'vitest';

import { formatShare, regionName } from './_helpers';
import { metricPercentile, webVitalGrade } from './Pages';
import { relativeTime } from './Sessions';

describe('formatShare', () => {
  it('writes a share with the decimals it has, up to two', () => {
    expect(formatShare(0.75)).toBe('75%');
    expect(formatShare(0.9997)).toBe('99.97%');
    expect(formatShare(0.5)).toBe('50%');
    expect(formatShare(1 / 3)).toBe('33.33%');
    expect(formatShare(0)).toBe('0%');
  });
});

describe('regionName', () => {
  it('names a region in the language of the product', () => {
    expect(regionName('BR', 'en-us')).toBe('Brazil');
    expect(regionName('de', 'en-us')).toBe('Germany');
    expect(regionName('JP', 'zh-cn')).toBe('日本');
  });

  it('leaves what is not a region code as it came', () => {
    expect(regionName('Unknown', 'en-us')).toBe('Unknown');
    expect(regionName('', 'en-us')).toBe('');
    expect(regionName('EU-West', 'zh-cn')).toBe('EU-West');
  });
});

describe('relativeTime', () => {
  const now = Date.parse('2026-10-13T03:00:00Z');
  const ago = (minutes: number) => (now - minutes * 60_000) * 1000;

  it('is said in the language of the product, not of the browser', () => {
    expect(relativeTime(ago(47), 'en-us', now)).toBe('47 minutes ago');
    expect(relativeTime(ago(47), 'zh-cn', now)).toBe('47分钟前');
    expect(relativeTime(ago(180), 'zh-cn', now)).toBe('3小时前');
    expect(relativeTime(ago(3 * 24 * 60), 'en-us', now)).toBe('3 days ago');
  });

  it('has nothing to say of a session with no start', () => {
    expect(relativeTime(undefined, 'en-us', now)).toBe('—');
  });
});

describe('the vitals of a page', () => {
  const point = (values: Record<string, number | null | undefined>) =>
    ({ ts_micros: 1, ...values }) as Parameters<typeof metricPercentile>[0][number];

  it('are the 75th percentile of the samples that carry them', () => {
    const points = [1000, 2000, 3000, 4000].map((lcp_ms) => point({ lcp_ms }));

    expect(metricPercentile(points, 'lcp_ms')).toBe(3000);
  });

  it('are not measured where no sample carries them: that is not a vital of zero', () => {
    const points = [point({ lcp_ms: 2000, inp_ms: null }), point({ lcp_ms: 2400 })];

    expect(metricPercentile(points, 'inp_ms')).toBeNull();
    expect(metricPercentile([], 'cls')).toBeNull();
  });

  it('grade a page by what was measured on it', () => {
    expect(webVitalGrade(2000, null, 0.05)).toBe('good');
    expect(webVitalGrade(3000, null, null)).toBe('needs_improvement');
    expect(webVitalGrade(2000, 600, 0.05)).toBe('poor');
    expect(webVitalGrade(null, null, null)).toBe('unknown');
    // A layout that never shifted is a measurement, and a good one.
    expect(webVitalGrade(null, null, 0)).toBe('good');
  });
});

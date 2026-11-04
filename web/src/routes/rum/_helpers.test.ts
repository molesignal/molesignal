import { describe, expect, it } from 'vitest';

import { formatDurationNs, formatShare, formatSize, regionName } from './_helpers';

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

  it('names Taiwan, Hong Kong and Macao as parts of China, as the map draws them', () => {
    expect(regionName('TW', 'zh-cn')).toBe('中国台湾');
    expect(regionName('tw', 'en-us')).toBe('Taiwan, China');
    expect(regionName('HK', 'zh-cn')).toBe('中国香港');
    expect(regionName('HK', 'en-us')).toBe('Hong Kong, China');
    expect(regionName('MO', 'zh-cn')).toBe('中国澳门');
    expect(regionName('MO', undefined)).toBe('Macao, China');
    expect(regionName('CN', 'zh-cn')).toBe('中国');
  });

  it('leaves what is not a region code as it came', () => {
    expect(regionName('Unknown', 'en-us')).toBe('Unknown');
    expect(regionName('', 'en-us')).toBe('');
    expect(regionName('EU-West', 'zh-cn')).toBe('EU-West');
  });
});

describe('formatDurationNs', () => {
  it('tells a duration the SDK sent in nanoseconds in the unit that reads best', () => {
    expect(formatDurationNs(850)).toBe('850 ns');
    expect(formatDurationNs(42_000)).toBe('42 µs');
    expect(formatDurationNs(120_000_000)).toBe('120 ms');
    expect(formatDurationNs(1_400_000_000)).toBe('1.4 s');
    expect(formatDurationNs(90_000_000_000)).toBe('1.5 min');
  });

  it('has nothing to say of a duration that is not there', () => {
    expect(formatDurationNs(undefined)).toBe('—');
    expect(formatDurationNs(Number.NaN)).toBe('—');
  });
});

describe('formatSize', () => {
  it('tells a size in bytes in the unit that reads best', () => {
    expect(formatSize(812)).toBe('812 B');
    expect(formatSize(4_300)).toBe('4.2 KB');
    expect(formatSize(1_400_000)).toBe('1.3 MB');
    expect(formatSize(undefined)).toBe('—');
  });
});

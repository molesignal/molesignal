import { describe, expect, it } from 'vitest';

import type { SourceLabel, SourceUsage } from '@/api/intakeUsage';

import {
  UNKNOWN_FIGURE,
  budgetStateRank,
  bytesFigure,
  countFigure,
  dimensionFromParam,
  formatBytes,
  limitFigure,
  limitParts,
  matchesSearch,
  parseLimit,
  parseWarnPercent,
  shareFigure,
  sourceCaption,
  sourceName,
  usedFigure,
  windowFromParam,
} from './model';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key}:${JSON.stringify(options)}` : key) as never;

const label = (fields: Partial<SourceLabel>): SourceLabel => ({
  kind: 'api_token',
  subkind: null,
  name: 'name',
  detail: null,
  missing: false,
  ...fields,
});

describe('figures', () => {
  it('says unknown, and never zero, for what is not known', () => {
    expect(bytesFigure(null)).toBe(UNKNOWN_FIGURE);
    expect(bytesFigure(undefined)).toBe(UNKNOWN_FIGURE);
    expect(countFigure(null)).toBe(UNKNOWN_FIGURE);
    expect(shareFigure(null)).toBe(UNKNOWN_FIGURE);
    expect(usedFigure(null)).toBe(UNKNOWN_FIGURE);
    expect(usedFigure({ usedRatio: null, usedComplete: true })).toBe(UNKNOWN_FIGURE);
    expect(UNKNOWN_FIGURE).toBe('–');
  });

  it('keeps a real zero', () => {
    expect(bytesFigure(0)).toBe('0 B');
    expect(countFigure(0)).toBe('0');
    expect(shareFigure(0)).toBe('0%');
    expect(usedFigure({ usedRatio: 0, usedComplete: true })).toBe('0%');
  });

  it('marks a lower bound', () => {
    expect(bytesFigure(1024, false)).toBe('≥ 1 KiB');
    expect(countFigure(5, false)).toBe('≥ 5');
    expect(usedFigure({ usedRatio: 1.5, usedComplete: false })).toBe('≥ 150%');
  });

  it('reads bytes in the units of the pages', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1,023 B');
    expect(formatBytes(1536)).toBe('1.5 KiB');
    expect(formatBytes(1024 ** 5)).toBe('1 PiB');
  });

  it('rounds the share used down, so that 100% means used up', () => {
    expect(usedFigure({ usedRatio: 0.999, usedComplete: true })).toBe('99%');
    expect(usedFigure({ usedRatio: 1, usedComplete: true })).toBe('100%');
  });

  it('says a sliver is one rather than rounding it to nothing', () => {
    expect(shareFigure(0.0004)).toBe('< 0.1%');
    expect(shareFigure(0.123)).toBe('12.3%');
    expect(shareFigure(1)).toBe('100%');
  });
});

describe('limits', () => {
  it('reads what was typed in whole MiB', () => {
    expect(parseLimit('5', 'MiB')).toBe(5);
    expect(parseLimit('5', 'GiB')).toBe(5120);
    expect(parseLimit('1.5', 'GiB')).toBe(1536);
    expect(parseLimit(' 2 ', 'TiB')).toBe(2 * 1024 * 1024);
  });

  it('refuses what is not a limit', () => {
    expect(parseLimit('', 'GiB')).toBeNull();
    expect(parseLimit('abc', 'GiB')).toBeNull();
    expect(parseLimit('-1', 'GiB')).toBeNull();
    expect(parseLimit('0', 'GiB')).toBeNull();
    expect(parseLimit('0.1', 'MiB')).toBeNull();
    expect(parseLimit('1025', 'TiB')).toBeNull();
  });

  it('says a limit in the largest unit that keeps it whole', () => {
    expect(limitParts(5)).toEqual({ text: '5', unit: 'MiB' });
    expect(limitParts(5120)).toEqual({ text: '5', unit: 'GiB' });
    expect(limitParts(1536)).toEqual({ text: '1536', unit: 'MiB' });
    expect(limitParts(1024 * 1024)).toEqual({ text: '1', unit: 'TiB' });
    expect(limitFigure(10 * 1024)).toBe('10 GiB');
  });

  it('reads back what it wrote', () => {
    for (const size of [1, 1536, 5120, 1024 * 1024, 1024 ** 3]) {
      const { text, unit } = limitParts(size);
      expect(parseLimit(text, unit)).toBe(size);
    }
  });

  it('takes a warning percentage up to 99', () => {
    expect(parseWarnPercent('0')).toBe(0);
    expect(parseWarnPercent('80')).toBe(80);
    expect(parseWarnPercent('99')).toBe(99);
    expect(parseWarnPercent('100')).toBeNull();
    expect(parseWarnPercent('8.5')).toBeNull();
    expect(parseWarnPercent('')).toBeNull();
  });
});

describe('the address', () => {
  it('falls back on what it does not know', () => {
    expect(dimensionFromParam('stream')).toBe('stream');
    expect(dimensionFromParam('nonsense')).toBe('credential');
    expect(dimensionFromParam(null)).toBe('credential');
    expect(windowFromParam('30d').key).toBe('30d');
    expect(windowFromParam('nonsense').key).toBe('24h');
    expect(windowFromParam(null).key).toBe('24h');
  });
});

describe('names', () => {
  it('words the sources that have no name of their own', () => {
    expect(sourceName(t, 'internal:', label({ kind: 'internal', name: '' }))).toBe('usage.source.internal');
    expect(sourceName(t, '', label({ kind: 'service', name: '' }))).toBe('usage.source.unnamed_service');
    expect(sourceName(t, '*', label({ kind: 'service', name: '*' }))).toBe('usage.source.other_services');
    expect(sourceName(t, 'checkout', label({ kind: 'service', name: 'checkout' }))).toBe('checkout');
  });

  it('says what kind of source it is and what tells it apart', () => {
    const token = sourceCaption(t, label({ subkind: 'rum_client', detail: 'mspat-abc' }));

    expect(token).toContain('usage.token_kinds.rum_client');
    expect(token).toContain('mspat-abc');
    expect(sourceCaption(t, label({ kind: 'user', detail: 'a@b.c' }))).toBe('usage.source.user · a@b.c');
    expect(sourceCaption(t, label({ kind: 'service' }))).toBe('');
  });

  it('finds a source by anything shown about it', () => {
    const row = {
      sourceKey: 'api_token:t-1',
      label: label({ name: 'Checkout-Prod', subkind: 'personal', detail: 'mspat-abc' }),
    } as SourceUsage;

    expect(matchesSearch(t, row, '')).toBe(true);
    expect(matchesSearch(t, row, 'checkout')).toBe(true);
    expect(matchesSearch(t, row, 'MSPAT-ABC')).toBe(true);
    expect(matchesSearch(t, row, 't-1')).toBe(true);
    expect(matchesSearch(t, row, 'search')).toBe(false);
  });
});

describe('order', () => {
  it('puts what wants attention first and what has no budget out of the way', () => {
    expect(budgetStateRank('exceeded')).toBeLessThan(budgetStateRank('warning') ?? Infinity);
    expect(budgetStateRank('warning')).toBeLessThan(budgetStateRank('ok') ?? Infinity);
    expect(budgetStateRank(null)).toBeNull();
    expect(budgetStateRank(undefined)).toBeNull();
  });
});

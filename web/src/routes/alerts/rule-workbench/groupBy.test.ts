import { describe, expect, it } from 'vitest';

import { countGroups, formatGroupBy, missingGroupColumns, parseGroupBy } from './groupBy';

const result = (columns: string[], rows: unknown[][]) => ({ columns, rows, scannedRows: rows.length, tookMs: 1 });

describe('split by columns', () => {
  it('reads the columns typed with commas of either width', () => {
    expect(parseGroupBy(' host, service.name，host ,, ')).toEqual(['host', 'service.name']);
    expect(parseGroupBy('')).toEqual([]);
    expect(formatGroupBy(['host', 'service.name'])).toBe('host, service.name');
    expect(formatGroupBy(undefined)).toBe('');
  });

  it('names the split columns a result with rows does not have', () => {
    const rows = result(['HOST', 'errors'], [['web-1', 3]]);
    expect(missingGroupColumns(rows, ['host', 'service'])).toEqual(['service']);
    // No rows, no groups: nothing to miss yet.
    expect(missingGroupColumns(result(['errors'], []), ['host'])).toEqual([]);
  });

  it('counts the groups the rows fall into', () => {
    const rows = result(
      ['host', 'env', 'errors'],
      [
        ['web-1', 'prod', 3],
        ['web-2', 'prod', 1],
        ['web-1', 'prod', 9],
        ['web-1', null, 2],
      ],
    );
    expect(countGroups(rows, ['host'])).toBe(2);
    expect(countGroups(rows, ['host', 'env'])).toBe(3);
    expect(countGroups(rows, ['missing'])).toBe(0);
  });
});

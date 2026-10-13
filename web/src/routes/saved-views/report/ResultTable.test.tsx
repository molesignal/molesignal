import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { setActiveTimePreference } from '@/lib/time';

import { cellText, columnWidths, ResultTable } from './ResultTable';

beforeEach(() => {
  setActiveTimePreference('Asia/Shanghai', 'iso_24h');
});

afterEach(() => {
  cleanup();
  setActiveTimePreference('', 'iso_24h');
});

describe('cellText', () => {
  it("reads the engine's time column as a time in the report's zone", () => {
    expect(cellText('_timestamp', Date.parse('2026-10-10T08:15:30Z') * 1_000)).toBe('2026-10-10 16:15:30');
  });

  it('leaves a number alone in a column the statement named itself', () => {
    // Only `_timestamp` is known to be microseconds; `ts` could be anything.
    expect(cellText('ts', 1_760_084_130_000_000)).toBe('1760084130000000');
    expect(cellText('_timestamp', '2026-10-10T08:15:30Z')).toBe('2026-10-10T08:15:30Z');
  });

  it('prints what the statement returned', () => {
    expect(cellText('service', 'checkout')).toBe('checkout');
    expect(cellText('errors', 42)).toBe('42');
    expect(cellText('ratio', 0.25)).toBe('0.25');
    expect(cellText('sampled', false)).toBe('false');
    expect(cellText('detail', { code: 502, hosts: ['a', 'b'] })).toBe('{"code":502,"hosts":["a","b"]}');
    expect(cellText('message', '')).toBe('');
  });

  it('marks a missing value', () => {
    expect(cellText('service', null)).toBe('—');
    expect(cellText('service', undefined)).toBe('—');
  });
});

describe('columnWidths', () => {
  it('asks for the longest thing in the column, the header included', () => {
    expect(
      columnWidths(
        ['service', 'n'],
        [
          ['api', '1234567'],
          ['checkout-gateway', '42'],
        ],
      ),
      // 16 and 7 characters, each with the cell's own padding and rule.
    ).toEqual([20, 11]);
    // A header longer than every value still fits on one line.
    expect(columnWidths(['error_count'], [['3']])).toEqual([15]);
  });

  it('keeps a narrow column wide enough to read and a long one from taking the page', () => {
    expect(columnWidths(['n', 'message'], [['1', 'x'.repeat(400)]])).toEqual([8, 52]);
  });

  it('counts the longest line of a value that runs over several', () => {
    expect(columnWidths(['stack'], [['Error: boom\n    at handler (server.js:10)\n    at run']])).toEqual([
      '    at handler (server.js:10)'.length + 4,
    ]);
  });
});

describe('ResultTable', () => {
  it('is as wide as its columns ask for and no wider than the page', () => {
    render(
      <ResultTable
        label="Results"
        columns={['service', 'errors']}
        rows={[
          ['checkout', 42],
          ['payments', 7],
        ]}
      />,
    );

    const table = screen.getByRole('table', { name: 'Results' });
    // 8 + 4 and 6 + 4 characters.
    expect(table.style.width).toBe('min(100%, 22ch)');
    expect(table.className).toContain('table-fixed');
    const shares = Array.from(table.querySelectorAll('col'), (col) => Number.parseFloat(col.style.width));
    expect(shares).toHaveLength(2);
    expect(shares[0]).toBeCloseTo((12 / 22) * 100, 5);
    expect(shares[0]! + shares[1]!).toBeCloseTo(100, 5);
  });

  it('prints every row under the columns the statement selected', () => {
    render(
      <ResultTable
        label="Results"
        // A statement can select the same name twice.
        columns={['service', 'service']}
        rows={[['checkout', 'payments'], ['cart']]}
      />,
    );

    const rows = screen.getAllByRole('row');
    expect(rows.map((row) => Array.from(row.querySelectorAll('th, td'), (cell) => cell.textContent))).toEqual([
      ['service', 'service'],
      ['checkout', 'payments'],
      // A row shorter than the header still has a cell under every column.
      ['cart', '—'],
    ]);
  });
});

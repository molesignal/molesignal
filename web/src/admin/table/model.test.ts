import { describe, expect, it } from 'vitest';

import { nextSort, rowsToCsv, runOverRows, sortRows } from './model';

describe('nextSort', () => {
  it('goes up, down, then back to the list order', () => {
    const up = nextSort(null, 'name');
    expect(up).toEqual({ key: 'name', direction: 'asc' });

    const down = nextSort(up, 'name');
    expect(down).toEqual({ key: 'name', direction: 'desc' });
    expect(nextSort(down, 'name')).toBeNull();
  });

  it('starts over on another column', () => {
    expect(nextSort({ key: 'name', direction: 'desc' }, 'age')).toEqual({ key: 'age', direction: 'asc' });
  });
});

describe('sortRows', () => {
  const rows = [
    { id: 'a', name: 'node-10', size: 5 },
    { id: 'b', name: 'node-9', size: null },
    { id: 'c', name: 'Node-1', size: 20 },
    { id: 'd', name: '', size: 5 },
  ];
  const ids = (list: typeof rows) => list.map((row) => row.id).join('');

  it('reads digits as numbers and ignores case', () => {
    expect(ids(sortRows(rows, (row) => row.name, 'asc'))).toBe('cbad');
    expect(ids(sortRows(rows, (row) => row.name, 'desc'))).toBe('abcd');
  });

  it('keeps what has no value last in both directions', () => {
    expect(ids(sortRows(rows, (row) => row.size, 'asc'))).toBe('adcb');
    expect(ids(sortRows(rows, (row) => row.size, 'desc'))).toBe('cadb');
  });

  it('keeps ties in the order of the list', () => {
    expect(ids(sortRows(rows, () => 1, 'desc'))).toBe('abcd');
  });

  it('does not touch the list it was given', () => {
    const before = ids(rows);
    sortRows(rows, (row) => row.name, 'asc');
    expect(ids(rows)).toBe(before);
  });
});

describe('rowsToCsv', () => {
  const columns = [
    { header: '名称', value: (row: { name: string; count: number | null }) => row.name },
    { header: 'count', value: (row: { name: string; count: number | null }) => row.count },
  ];

  it('opens with the UTF-8 mark and ends each row with CRLF', () => {
    expect(rowsToCsv(columns, [{ name: '规则', count: 3 }])).toBe('\uFEFF名称,count\r\n规则,3\r\n');
  });

  it('quotes what would break a cell', () => {
    expect(rowsToCsv(columns, [{ name: 'a,"b"\nc', count: null }])).toBe('\uFEFF名称,count\r\n"a,""b""\nc",\r\n');
  });

  it('keeps a spreadsheet from running a value as a formula', () => {
    expect(rowsToCsv(columns, [{ name: '=HYPERLINK("x")', count: -1 }])).toContain(`"'=HYPERLINK(""x"")",-1`);
  });
});

describe('runOverRows', () => {
  it('tries every row and counts what failed', async () => {
    const outcome = await runOverRows([1, 2, 3], async (row) => {
      if (row === 2) throw new Error('no');
    });

    expect(outcome.done).toBe(2);
    expect(outcome.failed).toBe(1);
    expect((outcome.firstError as Error).message).toBe('no');
  });
});

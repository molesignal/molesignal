import { describe, expect, it } from 'vitest';

import {
  fieldValueText,
  flattenRecord,
  isFilterableValue,
  logFieldRows,
  logFileName,
  matchFieldRows,
} from './model';

const RECORD = {
  _timestamp: 1_779_530_394_120_000,
  level: 'ERROR',
  'service.name': 'checkout-api',
  message: 'failed to reserve stock',
  trace_id: 'trace-1',
  error: { type: 'UpstreamTimeout', code: 504, upstream: { host: 'bank.example.com' } },
  flags: ['new_checkout', 'express_pay'],
  retry: false,
  note: null,
};

describe('the field list', () => {
  it('lays a record out leaf by leaf, a list as one value', () => {
    expect(flattenRecord({ a: 1, b: { c: 'x', d: { e: true } }, f: [1, 2] })).toEqual([
      { path: 'a', value: 1 },
      { path: 'b.c', value: 'x' },
      { path: 'b.d.e', value: true },
      { path: 'f', value: [1, 2] },
    ]);
  });

  it('leaves out what the head of the drawer already says in full: the moment and the message', () => {
    const rows = logFieldRows(RECORD, 'message').map((row) => row.path);

    expect(rows).toEqual([
      'level',
      'service.name',
      'trace_id',
      'error.type',
      'error.code',
      'error.upstream.host',
      'flags',
      'retry',
      'note',
    ]);
    // With no message of its own, a record keeps every field but the moment.
    expect(logFieldRows(RECORD, null).map((row) => row.path)).toContain('message');
  });

  it('finds a field by its name or by what it holds, whatever the case', () => {
    const rows = logFieldRows(RECORD, 'message');

    expect(matchFieldRows(rows, 'ERROR').map((row) => row.path)).toEqual([
      'level',
      'error.type',
      'error.code',
      'error.upstream.host',
    ]);
    expect(matchFieldRows(rows, 'bank').map((row) => row.path)).toEqual(['error.upstream.host']);
    expect(matchFieldRows(rows, 'express').map((row) => row.path)).toEqual(['flags']);
    expect(matchFieldRows(rows, '  ')).toHaveLength(rows.length);
    expect(matchFieldRows(rows, 'nothing-like-this')).toEqual([]);
  });

  it('writes a value as a query would, and nothing as the word for it', () => {
    expect(fieldValueText('x')).toBe('x');
    expect(fieldValueText(504)).toBe('504');
    expect(fieldValueText(false)).toBe('false');
    expect(fieldValueText(['a', 'b'])).toBe('["a","b"]');
    expect(fieldValueText(null)).toBe('null');
    expect(fieldValueText(undefined)).toBe('');
  });

  it('offers a filter only for a value there is', () => {
    expect(isFilterableValue('x')).toBe(true);
    expect(isFilterableValue(0)).toBe(true);
    expect(isFilterableValue(false)).toBe(true);
    expect(isFilterableValue(null)).toBe(false);
    expect(isFilterableValue('')).toBe(false);
  });
});

describe('logFileName', () => {
  it('names the file by the log\'s moment, or by its place when it has none', () => {
    expect(logFileName('2026-05-23 09:59:54', 5)).toBe('molesignal-log-2026-05-23-09-59-54.json');
    expect(logFileName('—', 5)).toBe('molesignal-log-5.json');
  });
});

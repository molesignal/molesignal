import { describe, expect, it } from 'vitest';

import type { LogEntry } from '../viewModel';
import { logPatterns, longestFixedRun, patternKey, patternTokens } from './model';

const entry = (message: string | null, level = 'INFO', service = 'checkout-api'): LogEntry => ({
  ts: '2026-05-23 09:59:54',
  micros: 1_779_530_394_120_000,
  level: level as LogEntry['level'],
  raw: { level, 'service.name': service, ...(message === null ? {} : { message }) },
});
const keyOf = (message: string) => patternKey(patternTokens(message));

describe('reading a message into a pattern', () => {
  it('keeps the words and opens the places that change', () => {
    expect(keyOf('payment authorized for order 8f31c2 in 182ms')).toBe(
      'payment authorized for order <any> in <number>ms',
    );
    expect(keyOf('request completed')).toBe('request completed');
    expect(keyOf('retrying (3) after 1.5s, gave up.')).toBe('retrying (<number>) after <number>s, gave up.');
  });

  it('tells an address, a moment and an identifier from a number', () => {
    expect(keyOf('GET https://bank.example.com/v2/charges from 10.0.3.17:8443')).toBe('GET <url> from <ip>');
    expect(keyOf('started at 2026-05-23T09:59:54.120Z took 12:00:01')).toBe('started at <time> took <time>');
    expect(keyOf('trace 7c1f0e0004a94b4d2e8f3a5b6c7d8e9f span 550e8400-e29b-41d4-a716-446655440000')).toBe(
      'trace <id> span <id>',
    );
  });

  it('takes letters after a number for a unit only when they are one', () => {
    expect(keyOf('used 1.5GB of 4gb at 99% for 30s, 250ms and 3h')).toBe(
      'used <number>GB of <number>gb at <number>% for <number>s, <number>ms and <number>h',
    );
    // An identifier that begins with digits is not so many of something.
    expect(keyOf('order 12ae and order 5ea shipped')).toBe('order <any> and order <any> shipped');
  });

  it('keeps the name of a named value and opens the value', () => {
    expect(keyOf('failed to reserve stock: sku=SKU-48213 requested=4 available=1 warehouse=eu-central-1a')).toBe(
      'failed to reserve stock: sku=<any> requested=<number> available=<number> warehouse=<any>',
    );
    // A value that is a word is part of what the message says.
    expect(keyOf('cache refresh status=ok method=GET')).toBe('cache refresh status=ok method=GET');
  });

  it('takes data written out at the end as one place, but not a word in brackets', () => {
    expect(keyOf('resolved feature flags {"new_checkout":true,"express_pay":false}')).toBe(
      'resolved feature flags <data>',
    );
    expect(keyOf('[worker] finished job [ok]')).toBe('[worker] finished job [ok]');
  });

  it('reads the first line of a message that goes on', () => {
    expect(keyOf('failed to connect\n    at Pool.acquire (pool.ts:214)\n    at main (index.ts:8)')).toBe(
      'failed to connect',
    );
  });

  it('stops at a length a pattern can still be read at', () => {
    const tokens = patternTokens(Array.from({ length: 80 }, (_, index) => `word${'a'.repeat(index % 3)}`).join(' '));

    expect(tokens).toHaveLength(49);
    expect(tokens.at(-1)?.text).toBe('…');
  });
});

describe('logPatterns', () => {
  it('groups the logs that say the same thing, the most of them first', () => {
    const rows = [
      entry('payment authorized for order 8f31c2 in 182ms', 'INFO', 'payments'),
      entry('order accepted'),
      entry('payment authorized for order 77aa01 in 95ms', 'INFO', 'payments'),
      entry('upstream timeout after 3 retries', 'ERROR', 'gateway'),
      entry('payment authorized for order 1b2c3d in 240ms', 'WARN', 'checkout-api'),
      entry(null),
    ];
    const { patterns, read, unread } = logPatterns(rows);

    expect(read).toBe(5);
    expect(unread).toBe(1);
    expect(patterns.map((pattern) => [pattern.key, pattern.count])).toEqual([
      ['payment authorized for order <any> in <number>ms', 3],
      ['upstream timeout after <number> retries', 1],
      ['order accepted', 1],
    ]);

    const [payments] = patterns;
    expect(payments?.rows).toEqual([0, 2, 4]);
    expect(payments?.share).toBeCloseTo(0.6);
    // The loudest level among its logs, and how they divide.
    expect(payments?.tone).toBe('warn');
    expect(payments?.levels).toEqual([
      { level: 'INFO', count: 2 },
      { level: 'WARN', count: 1 },
    ]);
    expect(payments?.sources).toEqual([
      { name: 'payments', count: 2 },
      { name: 'checkout-api', count: 1 },
    ]);
    expect(payments?.field).toBe('message');
  });

  it('opens a place where the logs differ by a word in three ways or more', () => {
    const { patterns } = logPatterns([
      entry('user alice signed in'),
      entry('user bob signed in'),
      entry('user carol signed in'),
      entry('user alice signed out'),
    ]);

    expect(patterns.map((pattern) => [pattern.key, pattern.count])).toEqual([
      ['user <any> signed in', 3],
      ['user alice signed out', 1],
    ]);
  });

  it('takes a word for a value where other logs have a value in its place', () => {
    const { patterns } = logPatterns([
      entry('payment authorized for order 8f31c2 in 182ms'),
      // Identifiers of the same kind that happen to be all letters, or all digits.
      entry('payment authorized for order ccdffc in 95ms'),
      entry('payment authorized for order 482910 in 240ms'),
    ]);

    expect(patterns.map((pattern) => [pattern.key, pattern.count])).toEqual([
      ['payment authorized for order <any> in <number>ms', 3],
    ]);
    // What a query can ask for is still what every one of them says.
    expect(patterns[0]?.fragment).toBe('payment authorized for order');
  });

  it('joins a number counted in one unit and in another as a number', () => {
    const { patterns } = logPatterns([entry('query took 250ms to plan'), entry('query took 2s to plan')]);

    expect(patterns.map((pattern) => [pattern.key, pattern.count])).toEqual([['query took <number> to plan', 2]]);
  });

  it('leaves two events that differ by a word as two', () => {
    const { patterns } = logPatterns([
      entry('order 17 was accepted'),
      entry('order 18 was rejected'),
      entry('order accepted'),
      entry('order rejected'),
      entry('order shipped'),
    ]);

    expect(patterns.map((pattern) => pattern.key).sort()).toEqual([
      'order <number> was accepted',
      'order <number> was rejected',
      'order accepted',
      'order rejected',
      'order shipped',
    ]);
  });

  it('never opens the word a message begins with', () => {
    const { patterns } = logPatterns([
      entry('started worker pool'),
      entry('stopped worker pool'),
      entry('resized worker pool'),
    ]);

    expect(patterns).toHaveLength(3);
  });

  it('has nothing to group when no log says anything', () => {
    expect(logPatterns([entry(null), entry(null)])).toEqual({ patterns: [], read: 0, unread: 2 });
    expect(logPatterns([])).toEqual({ patterns: [], read: 0, unread: 0 });
  });
});

describe('what a query can ask for', () => {
  it('is the longest run of words every log of the pattern has, as the logs write it', () => {
    expect(longestFixedRun(patternTokens('payment authorized for order 8f31c2 in 182ms'))).toBe(
      'payment authorized for order',
    );
    expect(longestFixedRun(patternTokens('failed to reserve stock: sku=SKU-48213 requested=4'))).toBe(
      'failed to reserve stock: sku=',
    );
    expect(longestFixedRun(patternTokens('took 182ms to answer the request'))).toBe('ms to answer the request');
  });

  it('is nothing when too little of the pattern is fixed to ask for', () => {
    expect(longestFixedRun(patternTokens('42 7 ok'))).toBe('');
    expect(longestFixedRun(patternTokens('{"a":1}'))).toBe('');
  });
});

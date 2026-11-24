import { describe, expect, it } from 'vitest';

import type { LogPatternReport } from '@/api/web';

import type { LogEntry } from '../viewModel';
import { patternTokens } from './model';
import { matchesPattern, serverPatternsOf } from './server';

function entry(second: number, message: string, service = 'payments'): LogEntry {
  const micros = second * 1_000_000;
  return { ts: String(second), micros, level: 'INFO', raw: { _timestamp: micros, level: 'INFO', 'service.name': service, message } };
}

const TIMEOUT = [
  { lead: '', trail: '', text: 'upstream', wild: null, unit: '' },
  { lead: '', trail: '', text: 'timeout', wild: null, unit: '' },
  { lead: '', trail: '', text: 'after', wild: null, unit: '' },
  { lead: '', trail: '', text: '3', wild: 'number', unit: 'ms' },
] as const;

function report(patterns: LogPatternReport['patterns'], sampled = false): LogPatternReport {
  return {
    stream: 'app',
    field: 'message',
    levelField: 'level',
    timeRange: { startTime: 0, endTime: 1 },
    logs: 4_000,
    read: 400,
    unread: 0,
    sampled,
    patternCount: patterns.length,
    patterns,
  };
}

describe('server patterns', () => {
  it('matches a message of the same words, anything at the open places', () => {
    expect(matchesPattern(patternTokens('upstream timeout after 90ms'), [...TIMEOUT])).toBe(true);
    expect(matchesPattern(patternTokens('upstream timeout before 90ms'), [...TIMEOUT])).toBe(false);
    expect(matchesPattern(patternTokens('upstream timeout'), [...TIMEOUT])).toBe(false);
    expect(matchesPattern(patternTokens('anything'), [])).toBe(false);
  });

  it('gives each pattern the logs read of it, its count over the range, and how it moved', () => {
    const rows = [entry(3, 'upstream timeout after 12ms', 'checkout'), entry(2, 'order accepted'), entry(1, 'upstream timeout after 40ms', 'checkout')];
    const [timeout, gone] = serverPatternsOf(
      report(
        [
          {
            pattern: 'upstream timeout after <number>ms',
            tokens: [...TIMEOUT],
            count: 40,
            share: 0.1,
            estimatedCount: 400,
            firstSeen: 1,
            lastSeen: 3,
            levels: [{ level: 'ERROR', count: 40 }],
            baselineCount: 0,
            change: 'new',
            fragment: 'upstream timeout after',
          },
          { pattern: 'disk almost full', tokens: [], count: 0, share: 0, firstSeen: 1, lastSeen: 1, levels: [], change: 'dropping', example: 'disk almost full' },
        ],
        true,
      ),
      rows,
    );

    expect(timeout).toMatchObject({ count: 400, rows: [0, 2], tone: 'error', change: 'new', field: 'message', fragment: 'upstream timeout after' });
    expect(timeout!.sources).toEqual([{ name: 'checkout', count: 2 }]);
    expect(gone).toMatchObject({ count: 0, rows: [], change: 'dropping', example: 'disk almost full' });
  });
});

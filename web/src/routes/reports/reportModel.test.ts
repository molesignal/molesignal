import { describe, expect, it } from 'vitest';

import {
  nextRunAtMicros,
  normalizeMicros,
  parseIntervalSeconds,
  parseRecipient,
  readReportMetadata,
  sanitizeFilename,
} from './reportModel';

describe('reportModel', () => {
  it('parses the interval syntax supported by the scheduler', () => {
    expect(parseIntervalSeconds('every:6h')).toBe(21_600);
    expect(parseIntervalSeconds('every:7d')).toBe(604_800);
    expect(parseIntervalSeconds('0 9 * * 1')).toBeNull();
  });

  it('computes the next interval run and respects paused reports', () => {
    expect(
      nextRunAtMicros({
        cron: 'every:1h',
        enabled: true,
        last_run_at_micros: 1_000_000,
      }),
    ).toBe(3_601_000_000);
    expect(
      nextRunAtMicros({
        cron: 'every:1h',
        enabled: false,
        last_run_at_micros: 1_000_000,
      }),
    ).toBeNull();
  });

  it('normalizes delivery timestamps to microseconds', () => {
    expect(normalizeMicros(1_700_000_000)).toBe(1_700_000_000_000_000);
    expect(normalizeMicros(1_700_000_000_000)).toBe(1_700_000_000_000_000);
    expect(normalizeMicros(null)).toBeNull();
  });

  it('accepts delivery targets that the backend can deliver', () => {
    expect(parseRecipient('ops@example.com')).toEqual({
      recipient: { kind: 'email', target: 'ops@example.com' },
      error: null,
    });
    expect(parseRecipient('https://hooks.example.com/report')).toEqual({
      recipient: { kind: 'webhook', target: 'https://hooks.example.com/report' },
      error: null,
    });
    expect(parseRecipient('#ops')).toEqual({ recipient: null, error: 'unsupported' });
  });

  it('reads metadata defensively and sanitizes download names', () => {
    expect(readReportMetadata({ preset: 'previous-24-hours', description: ' Daily ' })).toEqual({
      preset: 'previous-24-hours',
      timezone: 'Asia/Shanghai',
      description: 'Daily',
    });
    expect(sanitizeFilename('Weekly / Ops: report')).toBe('Weekly-Ops-report');
  });
});

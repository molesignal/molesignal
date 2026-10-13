import type * as rumIntakeApi from '@/api/rumIntake';

import type { SourceSignalSummary } from '../datasourceModel';

/** How far back the receipt of a source is looked for. */
export const RECEIPT_WINDOW_SECS = 60 * 60;

export interface HealthProbe {
  ok: boolean;
  latencyMs: number;
  message?: string;
}

export type ValidationState =
  | { kind: 'idle' }
  | { kind: 'pending' }
  | { kind: 'success' | 'warning'; health: HealthProbe; summary: SourceSignalSummary }
  | { kind: 'error'; message: string };

export async function probeHealth(): Promise<HealthProbe> {
  const start = performance.now();

  try {
    const response = await fetch('/api/v1/healthz', { credentials: 'include' });
    const latencyMs = Math.round(performance.now() - start);

    return response.ok
      ? { ok: true, latencyMs }
      : { ok: false, latencyMs, message: `HTTP ${response.status}` };
  } catch (error) {
    return {
      ok: false,
      latencyMs: Math.round(performance.now() - start),
      message: (error as Error).message,
    };
  }
}

/** What a RUM application has sent, in the shape the other signals are summarized in. */
export function rumSourceSummary(
  receipt: rumIntakeApi.RumReceiptSummary = { rows: 0, lastReceivedAtMicros: null },
): SourceSignalSummary {
  const active = receipt.rows > 0 && receipt.lastReceivedAtMicros != null;

  return {
    status: active ? 'healthy' : 'no_data',
    rows: receipt.rows,
    storedBytes: 0,
    lastReceivedAtMicros: receipt.lastReceivedAtMicros,
    activeSignals: active ? 1 : 0,
    expectedSignals: 1,
    streamNames: active ? ['rum_errors'] : [],
  };
}

export function formatRelativeMicros(micros: number | null | undefined, locale: string): string {
  if (!micros) return '—';

  const deltaSeconds = Math.round(micros / 1000 - Date.now()) / 1000;
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });

  if (Math.abs(deltaSeconds) < 60) return formatter.format(Math.round(deltaSeconds), 'second');

  const minutes = deltaSeconds / 60;
  if (Math.abs(minutes) < 60) return formatter.format(Math.round(minutes), 'minute');

  const hours = minutes / 60;
  if (Math.abs(hours) < 24) return formatter.format(Math.round(hours), 'hour');

  return formatter.format(Math.round(hours / 24), 'day');
}

export function formatBytes(value: number): string {
  if (value < 1024) return `${Math.round(value)} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  return `${(value / 1024 ** 3).toFixed(2)} GiB`;
}

export function formatCount(value: number): string {
  if (value < 1_000) return `${Math.round(value)}`;
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

export function formatEventRate(rows: number, windowSecs: number): string {
  if (rows <= 0 || windowSecs <= 0) return '—';

  const perSecond = rows / windowSecs;
  if (perSecond >= 1) return `${formatCount(perSecond)}/s`;

  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)}/min`;

  return `${formatCount(perMinute * 60)}/h`;
}

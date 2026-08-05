import * as React from 'react';

import { LogStream } from '@/viz/log/LogStream';
import type { LogRow } from '@/viz/log/types';

/**
 * Demo route for log-stream perf testing
 * (web-playwright-runtime).
 *
 * URL: `/_demo/log?rows=1000000`
 *
 * Generates `N` synthetic LogRow objects into a Float-friendly array and
 * passes them straight to LogStream so the virtualizer drives the scroll
 * benchmark without any network or NDJSON parsing in the hot path.
 */
export function LogStreamDemo() {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const n = clamp(parseInt(params.get('rows') ?? '1000000', 10) || 1_000_000, 1, 5_000_000);

  const rows = React.useMemo(() => buildRows(n), [n]);

  return (
    <div className="flex h-screen flex-col">
      <header className="border-b border-border px-3 py-1 font-sans text-type-micro text-muted-foreground">
        /_demo/log · {n.toLocaleString()} rows
      </header>
      <div className="flex-1">
        <LogStream rows={rows} />
      </div>
    </div>
  );
}

function buildRows(n: number): LogRow[] {
  const LEVELS: Array<LogRow['level']> = ['info', 'info', 'info', 'warn', 'error', 'debug'];
  const SERVICES = ['web', 'api', 'auth', 'db', 'cache', 'queue', 'worker'];
  const MSG = [
    'GET / 200 in 18ms',
    'POST /api/users 201 in 42ms',
    'cache miss for key user:42',
    'connection pool drained, queuing request',
    'upstream timeout after 5s',
    'invalidating session token',
  ];
  const t0 = Date.UTC(2026, 4, 23, 10, 0, 0) * 1000; // microseconds
  const out: LogRow[] = new Array(n);
  for (let i = 0; i < n; i++) {
    out[i] = {
      _timestamp: t0 + i * 1000,
      level: LEVELS[i % LEVELS.length]!,
      service: SERVICES[i % SERVICES.length]!,
      message: MSG[i % MSG.length]!,
    };
  }
  return out;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

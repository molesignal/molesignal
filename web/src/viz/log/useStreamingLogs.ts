import * as React from 'react';

import { useAuthStore } from '@/stores/auth';

import type { LogRow } from './types';

interface StreamingOptions {
  url: string;
  body?: Record<string, unknown>;
  enabled?: boolean;
  /** Ring buffer cap. Default 100_000. */
  maxRows?: number;
}

interface StreamingState {
  rows: LogRow[];
  isConnected: boolean;
  error: Error | null;
}

/**
 * Streaming NDJSON consumer using fetch + ReadableStream. Each newline-
 * delimited frame is parsed as JSON and appended to a ring buffer.
 * Uses `useSyncExternalStore`-style API by maintaining the rows in a ref
 * and notifying via state version, so 1M-row appends do not cascade into
 * React reconciliation per row.
 */
export function useStreamingLogs({ url, body, enabled = true, maxRows = 100_000 }: StreamingOptions): StreamingState {
  const rowsRef = React.useRef<LogRow[]>([]);
  const [, setVersion] = React.useState(0);
  const [isConnected, setConnected] = React.useState(false);
  const [error, setError] = React.useState<Error | null>(null);
  const token = useAuthStore((s) => s.token);

  React.useEffect(() => {
    if (!enabled) return;
    const ac = new AbortController();
    rowsRef.current = [];
    setVersion((v) => v + 1);
    setConnected(false);
    setError(null);

    (async () => {
      try {
        const resp = await fetch(url, {
          method: body ? 'POST' : 'GET',
          headers: {
            Accept: 'application/x-ndjson',
            ...(body ? { 'content-type': 'application/json' } : {}),
            ...(token ? { authorization: `Bearer ${token}` } : {}),
          },
          ...(body && { body: JSON.stringify(body) }),
          signal: ac.signal,
        });
        if (!resp.ok || !resp.body) {
          throw new Error(`stream failed: ${resp.status}`);
        }
        setConnected(true);
        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let buf = '';
        let scheduled = false;
        const scheduleNotify = () => {
          if (scheduled) return;
          scheduled = true;
          globalThis.requestAnimationFrame(() => {
            scheduled = false;
            setVersion((v) => v + 1);
          });
        };
         
        while (true) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += decoder.decode(value, { stream: true });
          let nl = buf.indexOf('\n');
          while (nl >= 0) {
            const line = buf.slice(0, nl).trim();
            buf = buf.slice(nl + 1);
            nl = buf.indexOf('\n');
            if (!line) continue;
            try {
              const obj = JSON.parse(line) as LogRow | { meta: unknown };
              if ('_timestamp' in obj) {
                rowsRef.current.push(obj as LogRow);
                if (rowsRef.current.length > maxRows) {
                  rowsRef.current.splice(0, rowsRef.current.length - maxRows);
                }
                scheduleNotify();
              }
            } catch {
              /* skip malformed line */
            }
          }
        }
      } catch (e) {
        if ((e as Error).name !== 'AbortError') setError(e as Error);
      } finally {
        setConnected(false);
      }
    })();
    return () => ac.abort();
    // body is intentionally serialised in the dependency array — restart the
    // stream whenever the request payload changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url, JSON.stringify(body ?? {}), enabled, maxRows, token]);

  return { rows: rowsRef.current, isConnected, error };
  // NOTE: rows is a stable reference; subscribers should pair it with `version`
  // to know when to read. For now we re-export the snapshot directly; callers
  // that paint canvases would prefer the version.
  // (LogStream below renders via virtualizer that reads .length each cycle.)
}

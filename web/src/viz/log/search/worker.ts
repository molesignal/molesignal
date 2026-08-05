/**
 * searchWorker — substring filter for LogStream rows.
 *
 * Strategy:
 *   - `runSearch(rows, query)` is the pure-function entry point used by the worker
 *     and the inline (main-thread) path.
 *   - `searchLogs(rows, query)` dispatches:
 *       - rows ≤ 10k → inline (cheap; avoids worker overhead)
 *       - rows  > 10k → off-thread worker via dynamic Worker import
 *   - Match: case-insensitive substring across rendered fields (message, level,
 *     service, trace_id, plus stringified raw).
 *
 * Returns indices of matching rows so the virtualizer can map back without
 * cloning row payloads.
 *
 * Spec: web-investigation-shell.
 */

export interface LogRow {
  message?: string;
  level?: string;
  service?: string;
  trace_id?: string;
  raw?: unknown;
}

/** Pure synchronous search — used inline + inside the worker. */
export function runSearch(rows: ReadonlyArray<LogRow>, query: string): number[] {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return rows.map((_, i) => i);
  const out: number[] = [];
  for (let i = 0; i < rows.length; i++) {
    if (matchesRow(rows[i]!, q)) out.push(i);
  }
  return out;
}

function matchesRow(row: LogRow, q: string): boolean {
  if (row.message && row.message.toLowerCase().includes(q)) return true;
  if (row.level && row.level.toLowerCase().includes(q)) return true;
  if (row.service && row.service.toLowerCase().includes(q)) return true;
  if (row.trace_id && row.trace_id.toLowerCase().includes(q)) return true;
  if (row.raw != null) {
    try {
      const s = typeof row.raw === 'string' ? row.raw : JSON.stringify(row.raw);
      if (s.toLowerCase().includes(q)) return true;
    } catch {
      // ignore stringify failures
    }
  }
  return false;
}

const WORKER_THRESHOLD = 10_000;

let workerInstance: Worker | null = null;
let workerRequestId = 0;
const pending = new Map<number, (indices: number[]) => void>();

function getWorker(): Worker | null {
  if (typeof Worker === 'undefined') return null;
  if (workerInstance) return workerInstance;
  try {
    workerInstance = new Worker(new URL('./worker.bundle.ts', import.meta.url), {
      type: 'module',
    });
    workerInstance.onmessage = (e: MessageEvent<{ id: number; indices: number[] }>) => {
      const resolve = pending.get(e.data.id);
      if (resolve) {
        pending.delete(e.data.id);
        resolve(e.data.indices);
      }
    };
    workerInstance.onerror = () => {
      // Drop worker; fall back to inline next call.
      workerInstance?.terminate();
      workerInstance = null;
      pending.forEach((resolve) => resolve([]));
      pending.clear();
    };
    return workerInstance;
  } catch {
    return null;
  }
}

/**
 * Dispatch substring search. Resolves with indices of matching rows.
 * Always falls back to inline if the worker is unavailable or the row count
 * is below the threshold.
 */
export async function searchLogs(
  rows: ReadonlyArray<LogRow>,
  query: string,
): Promise<number[]> {
  if (rows.length <= WORKER_THRESHOLD) {
    return runSearch(rows, query);
  }
  const w = getWorker();
  if (!w) return runSearch(rows, query);
  return new Promise<number[]>((resolve) => {
    const id = ++workerRequestId;
    pending.set(id, resolve);
    w.postMessage({ id, rows, query });
  });
}

/** Cleanup hook for tests / hot reload. */
export function disposeSearchWorker(): void {
  workerInstance?.terminate();
  workerInstance = null;
  pending.clear();
}

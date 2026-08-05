/**
 * Worker entry that delegates to `runSearch`. Vite picks this up via the
 * `new Worker(new URL('./worker.bundle.ts', import.meta.url), { type: 'module' })`
 * call in `worker.ts`.
 */

import { runSearch, type LogRow } from './worker';

self.onmessage = (e: MessageEvent<{ id: number; rows: LogRow[]; query: string }>) => {
  const indices = runSearch(e.data.rows, e.data.query);
  (self as unknown as Worker).postMessage({ id: e.data.id, indices });
};

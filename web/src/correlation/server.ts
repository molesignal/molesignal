import * as webApi from '@/api/web';

const SERVER_TIMEOUT_MS = 400;

let serverTimeoutCount = 0;

export function getServerTimeoutCount(): number {
  return serverTimeoutCount;
}

/**
 * Try to delegate correlation derivation to the backend. Times out at 400ms
 * and resolves with `null` so callers fall back to the static client provider.
 */
export async function fetchServerCorrelation(
  from: string,
  to: string,
  ctx: Record<string, unknown>,
): Promise<webApi.CorrelationContext | null> {
  const ac = new AbortController();
  const timer = globalThis.setTimeout(() => ac.abort(), SERVER_TIMEOUT_MS);
  try {
    return await webApi.correlation(from, to, ctx, ac.signal);
  } catch (err) {
    if ((err as Error).name === 'AbortError' || (err as Error).name === 'CanceledError') {
      serverTimeoutCount += 1;
       
      console.warn('correlation_server_timeout', { from, to });
    }
    return null;
  } finally {
    globalThis.clearTimeout(timer);
  }
}

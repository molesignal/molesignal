import { toApiError } from '@/lib/http';

/**
 * Reads of a stream that has not been written to yet.
 *
 * A stream is created by its first write. Until then a read is turned away:
 * the query planner says `stream not found: <name>`, the stream catalog says
 * "stream `<name>` of type `<type>` does not exist" (HTTP 404). For a list or
 * an aggregate that means "no data yet", not a failure, so the page shows its
 * empty state and how to start sending instead of an error.
 */
export function isMissingStreamError(error: unknown): boolean {
  const { message } = toApiError(error);
  return (
    /stream not found|unknown stream/i.test(message) ||
    /stream `[^`]+`(?: of type `[^`]+`)? does not exist/i.test(message)
  );
}

/** Runs a read and answers `empty()` when the stream it reads is not there yet. */
export async function whenStreamExists<T>(
  read: () => Promise<T>,
  empty: () => T,
): Promise<T> {
  try {
    return await read();
  } catch (error) {
    if (isMissingStreamError(error)) return empty();
    throw error;
  }
}

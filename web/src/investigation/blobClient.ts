import * as webApi from '@/api/web';

/**
 * For investigations whose serialized stack exceeds 4 KB, swap the heavy
 * payload for a server-side blob and store only its id in the URL.
 *
 * Backend endpoint lives at /api/v1/web/investigation/blob (POST to store,
 * GET to read). Payload is opaque JSON with a 7-day TTL enforced by the
 * compactor sweeper task.
 */

const URL_LENGTH_LIMIT = 4096;

export function shouldUseBlob(serialized: string): boolean {
  return serialized.length > URL_LENGTH_LIMIT;
}

export async function storeBlob(payload: Record<string, unknown>): Promise<string> {
  const ref = await webApi.storeInvestigationBlob(payload);
  return ref.blob_id;
}

export async function fetchBlob(id: string): Promise<Record<string, unknown>> {
  return webApi.fetchInvestigationBlob(id);
}

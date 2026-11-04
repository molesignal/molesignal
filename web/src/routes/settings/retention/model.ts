import type { AuditRetentionPreview, StreamRetentionPreview } from '@/api/instance';

/** What is known about a retention change that is typed but not saved. */
export type RetentionPreviewState<T> =
  | { status: 'idle' }
  | { status: 'pending' }
  | { status: 'failed'; retry: () => void }
  | { status: 'ready'; preview: T };

/** What Save does: saves, asks, or asks for the new days typed back. */
export type RetentionSaveStep = 'save' | 'confirm' | 'type';

/**
 * Past this much a shorter default retention asks for the days typed back. A deployment that has
 * only tried the product stays under both; one with data worth keeping is over them at once.
 */
export const LARGE_STREAM_IMPACT = { rows: 1_000_000, storedBytes: 1024 ** 3 } as const;

/** Audit records are few and they are evidence, so far fewer of them count as a lot. */
export const LARGE_AUDIT_IMPACT_ROWS = 1_000;

/** Whether `days` keeps less than `stored`. 0 keeps for good, the longest there is. */
export function isShortening(stored: number, days: number): boolean {
  if (days === 0) return false;
  return stored === 0 || days < stored;
}

function saveStep<T>(
  state: RetentionPreviewState<T>,
  isNothing: (preview: T) => boolean,
  isLarge: (preview: T) => boolean,
): RetentionSaveStep {
  if (state.status === 'idle') return 'save';
  // What cannot be told is treated as the most it could be.
  if (state.status !== 'ready') return 'type';
  if (isNothing(state.preview)) return 'save';
  return isLarge(state.preview) ? 'type' : 'confirm';
}

export function streamSaveStep(state: RetentionPreviewState<StreamRetentionPreview>): RetentionSaveStep {
  return saveStep(
    state,
    (preview) => preview.streamCount === 0 && preview.unavailableStreams === 0,
    (preview) =>
      preview.unavailableStreams > 0 ||
      preview.rows >= LARGE_STREAM_IMPACT.rows ||
      preview.storedBytes >= LARGE_STREAM_IMPACT.storedBytes,
  );
}

export function auditSaveStep(state: RetentionPreviewState<AuditRetentionPreview>): RetentionSaveStep {
  return saveStep(
    state,
    (preview) => preview.rows === 0,
    (preview) => preview.rows >= LARGE_AUDIT_IMPACT_ROWS,
  );
}

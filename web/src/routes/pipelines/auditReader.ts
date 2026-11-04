import type { ScheduledPipeline } from '@/api/pipelines';

/**
 * The name of the person a pipeline reads `_audit` as: the display name, the email of someone who
 * has not set one, and the id alone when the user no longer exists. `null` when it reads as nobody,
 * which is every pipeline that does not read `_audit`.
 */
export function auditReaderName(pipeline: ScheduledPipeline | null | undefined): string | null {
  return (
    pipeline?.auditReader?.displayName.trim() ||
    pipeline?.auditReader?.email.trim() ||
    pipeline?.auditReaderId ||
    null
  );
}

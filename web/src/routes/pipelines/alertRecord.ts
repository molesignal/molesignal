import type { TFunction } from 'i18next';

import type { AlertPipelineRecord, ScheduledPipeline } from '@/api/pipelines';

import { executionMode } from './PipelineGraph/model';

/** The UTC day of a moment, as the backend counts an alert pipeline's events by it. */
function utcDay(millis: number): string {
  return new Date(millis).toISOString().slice(0, 10);
}

/** An alert pipeline's record with today's counts filled in. */
export type TodaysAlertRecord = AlertPipelineRecord & { processed: number; dropped: number; failed: number };

/**
 * An alert pipeline's counts of today (UTC), all zero when it saw nothing
 * today; `null` for a pipeline that is not an alert pipeline.
 */
export function todaysAlertRecord(pipeline: ScheduledPipeline, now: number = Date.now()): TodaysAlertRecord | null {
  if (executionMode(pipeline) !== 'alert') return null;

  const record = pipeline.alertRecord ?? {};
  const today = record.day === utcDay(now);

  return {
    ...record,
    processed: today ? (record.processed ?? 0) : 0,
    dropped: today ? (record.dropped ?? 0) : 0,
    failed: today ? (record.failed ?? 0) : 0,
  };
}

/** Today's counts of an alert pipeline in a line: processed, dropped, failed. */
export function alertRecordLine(pipeline: ScheduledPipeline, t: TFunction<'pipelines'>): string {
  const record = todaysAlertRecord(pipeline);
  if (!record) return '';

  return t('alert_mode.record_line', {
    processed: record.processed,
    dropped: record.dropped,
    failed: record.failed,
  });
}

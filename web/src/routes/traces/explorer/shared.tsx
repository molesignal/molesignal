import type { TFunction } from 'i18next';

import { zonedMoment } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { HTTP_STATUS_TEXT, httpStatusTone, OUTCOME_TEXT, type RequestOutcome } from '@/viz/trace/model/span';

export function formatTraceStart(startNs: number, tz: string): string {
  if (!Number.isFinite(startNs) || startNs <= 0) return '-';
  const date = zonedMoment(Math.floor(startNs / 1_000_000), tz);
  return date.isValid() ? date.format('HH:mm:ss.SSS') : '-';
}

/**
 * How a trace or a span ended, as a word in its colour: a refused request is still not an answer.
 * An answer is in the body weight and what went wrong in the strong one, so a page of answers
 * does not read as a column of emphasis.
 */
export function OutcomeWord({ outcome, t }: { outcome: RequestOutcome; t: TFunction<'traces'> }) {
  return (
    <span className={cn('truncate', outcome === 'ok' ? 'font-body' : 'font-strong', OUTCOME_TEXT[outcome])}>
      {t(outcome === 'ok' ? 'explore.table.status_ok' : 'explore.table.status_error')}
    </span>
  );
}

/** The HTTP status a request answered with, in the colour of its class, a 4xx or 5xx in the strong weight; a dash where it kept none. */
export function HttpStatusCode({ status }: { status: number | null | undefined }) {
  if (!status) return <span className="text-tx-4">—</span>;
  const tone = httpStatusTone(status);
  return (
    <span className={cn('tabular-nums', tone === 'client' || tone === 'server' ? 'font-strong' : 'font-body', HTTP_STATUS_TEXT[tone])}>
      {status}
    </span>
  );
}

import { useTranslation } from 'react-i18next';

import type {
  MissingRuns,
  MonitorRevision,
  SyntheticAvailability,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import { Readout } from '@/shell/readout/Readout';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import { formatRelativeMicros } from '@/time/relative';

import { formatDuration, formatPercent, scheduleText } from '../../model';
import {
  STATE_COLOR,
  STATE_TEXT,
  availabilityTone,
  decidedRuns,
  lastFailureAt,
  latestError,
  unhealthySince,
} from '../../summary';

/**
 * How one check stands: passing or not, since when, and what the answer rests
 * on. The verdict is the check's own state, or that it gets no results when
 * enough of its locations stopped running it. Beside it are how its latest
 * runs went, how long they took, and how often and from where it runs.
 */
export function CheckStanding({
  monitor,
  revision,
  results,
  availability,
  missingRuns,
  locationName,
  nowMicros,
}: {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
  /** The latest runs: what the check says now. */
  results: readonly SyntheticResult[];
  /** The runs over the range chosen: how far the check can be relied on. */
  availability: SyntheticAvailability | undefined;
  /** Where the check is missing runs. */
  missingRuns: MissingRuns | undefined;
  locationName: (id: string) => string;
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const onSchedule = monitor.lifecycle === 'active';
  const noData = onSchedule && missingRuns?.noData === true;
  const runs = decidedRuns(results);
  const total = availability?.total;
  const judged = total ? total.available + total.failing : 0;
  const passRate = total?.availability ?? undefined;
  const steps = availability?.buckets ?? [];
  const micros = (millis: number | null | undefined) =>
    millis === null || millis === undefined ? undefined : millis * 1000;
  const since = unhealthySince(results);
  const lastFailure = lastFailureAt(results);
  const unwell = onSchedule && ['failing', 'flaky', 'degraded'].includes(monitor.state);
  const when = (micros: number) => formatRelativeMicros(micros, i18n.language, nowMicros);
  const missing = missingRuns?.locations ?? [];

  const detail = !onSchedule
    ? t('detail.paused_detail')
    : noData
      ? t('detail.no_data_detail', {
          locations: missing.map((location) => locationName(location.locationId)).join(', '),
          count: Math.max(0, ...missing.map((location) => location.missedRuns)),
        })
    : runs.length === 0
      ? t('detail.never_ran')
      : unwell
        ? since !== undefined
          ? t('detail.since', { when: when(since) })
          : undefined
        : lastFailure !== undefined
          ? t('detail.last_failure', { when: when(lastFailure) })
          : t('detail.no_failures', { count: runs.length });
  // Why the latest of the missed runs did not happen, as the run says it.
  const skipped = noData
    ? results.find(
        (result) =>
          result.outcome === 'skipped' &&
          missing.some((location) => location.locationId === result.locationId),
      )?.metadata.skip_reason
    : undefined;
  const error = noData
    ? skipped && t(`results.skip_reasons.${skipped}`, { defaultValue: skipped })
    : unwell
      ? latestError(results)
      : undefined;
  const heartbeat = revision.schedule.kind === 'heartbeat';
  const standing = !onSchedule
    ? monitor.lifecycle === 'draft'
      ? 'draft'
      : 'paused'
    : noData
      ? 'no_data'
      : monitor.state;

  return (
    <Statement
      label={t('detail.standing_label')}
      tone={onSchedule && !noData ? STATE_COLOR[monitor.state] : 'var(--tx-3)'}
      eyebrow={t(`kinds.${monitor.kind}`)}
      title={t(`check_states.${standing}`)}
      titleClassName={onSchedule && !noData ? STATE_TEXT[monitor.state] : 'text-tx-0'}
      detail={detail}
      dataAttributes={{ 'data-check-standing': onSchedule ? standing : monitor.lifecycle }}
      readouts={
        <>
          <Readout
            className={STATEMENT_CELL}
            riseIndex={1}
            label={t('standing.availability')}
            value={formatPercent(passRate)}
            tone={availabilityTone(passRate)}
            trend={
              steps.length > 1 ? { values: steps.map((step) => step.availability) } : undefined
            }
            detail={
              judged > 0 ? t('standing.of_runs', { count: judged }) : t('standing.no_runs')
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={2}
            label={t('detail.response_time')}
            value={formatDuration(micros(total?.p50Ms))}
            trend={
              steps.length > 1
                ? { values: steps.map((step) => step.p50Ms), zeroBased: true }
                : undefined
            }
            detail={
              total?.p95Ms !== null && total?.p95Ms !== undefined
                ? t('detail.p95', { value: formatDuration(micros(total.p95Ms)) })
                : undefined
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={3}
            label={t(heartbeat ? 'detail.schedule_heartbeat' : 'detail.schedule')}
            value={scheduleText(revision.schedule)}
            detail={
              heartbeat
                ? undefined
                : t('detail.schedule_detail', {
                    count: revision.locationIds.length,
                    failures: revision.consecutiveFailures,
                  })
            }
          />
        </>
      }
    >
      {error && (
        <p className="mt-[10px] line-clamp-3 text-pretty font-sans text-xs leading-relaxed text-tx-1">
          {error}
        </p>
      )}
    </Statement>
  );
}

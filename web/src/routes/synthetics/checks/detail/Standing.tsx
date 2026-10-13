import { useTranslation } from 'react-i18next';

import type { MonitorRevision, SyntheticMonitor, SyntheticResult } from '@/api/synthetics';
import { Readout } from '@/shell/readout/Readout';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import { formatRelativeMicros } from '@/time/relative';

import {
  formatDuration,
  formatPercent,
  percentile,
  resultDurationMicros,
  scheduleText,
} from '../../model';
import {
  STATE_COLOR,
  STATE_TEXT,
  availability,
  availabilityTone,
  availabilityTrend,
  decidedRuns,
  lastFailureAt,
  latestError,
  unhealthySince,
} from '../../summary';

/**
 * How one check stands: passing or not, since when, and what the answer rests
 * on. The verdict is the check's own state. Beside it are how its latest runs
 * went, how long they took, and how often and from where it runs.
 */
export function CheckStanding({
  monitor,
  revision,
  results,
  nowMicros,
}: {
  monitor: SyntheticMonitor;
  revision: MonitorRevision;
  results: readonly SyntheticResult[];
  nowMicros: number;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const onSchedule = monitor.lifecycle === 'active';
  const runs = decidedRuns(results);
  const passRate = availability(results);
  const durations = runs
    .map(resultDurationMicros)
    .filter((value): value is number => value !== undefined);
  const since = unhealthySince(results);
  const lastFailure = lastFailureAt(results);
  const unwell = onSchedule && ['failing', 'flaky', 'degraded'].includes(monitor.state);
  const when = (micros: number) => formatRelativeMicros(micros, i18n.language, nowMicros);

  const detail = !onSchedule
    ? t('detail.paused_detail')
    : runs.length === 0
      ? t('detail.never_ran')
      : unwell
        ? since !== undefined
          ? t('detail.since', { when: when(since) })
          : undefined
        : lastFailure !== undefined
          ? t('detail.last_failure', { when: when(lastFailure) })
          : t('detail.no_failures', { count: runs.length });
  const error = unwell ? latestError(results) : undefined;
  const heartbeat = revision.schedule.kind === 'heartbeat';

  return (
    <Statement
      label={t('detail.standing_label')}
      tone={onSchedule ? STATE_COLOR[monitor.state] : 'var(--tx-3)'}
      eyebrow={t(`kinds.${monitor.kind}`)}
      title={t(`check_states.${onSchedule ? monitor.state : monitor.lifecycle === 'draft' ? 'draft' : 'paused'}`)}
      titleClassName={onSchedule ? STATE_TEXT[monitor.state] : 'text-tx-0'}
      detail={detail}
      dataAttributes={{ 'data-check-standing': onSchedule ? monitor.state : monitor.lifecycle }}
      readouts={
        <>
          <Readout
            className={STATEMENT_CELL}
            riseIndex={1}
            label={t('standing.availability')}
            value={formatPercent(passRate)}
            tone={availabilityTone(passRate)}
            trend={runs.length > 1 ? { values: availabilityTrend(results) } : undefined}
            detail={
              runs.length > 0
                ? t('standing.of_runs', { count: runs.length })
                : t('standing.no_runs')
            }
          />
          <Readout
            className={STATEMENT_CELL}
            riseIndex={2}
            label={t('detail.response_time')}
            value={formatDuration(percentile(durations, 0.5))}
            trend={
              durations.length > 1
                ? // Oldest first, so the line ends at the latest run.
                  { values: [...durations].reverse(), zeroBased: true }
                : undefined
            }
            detail={
              durations.length > 0
                ? t('detail.p95', { value: formatDuration(percentile(durations, 0.95)) })
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
                    count: revision.location_ids.length,
                    failures: revision.consecutive_failures,
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

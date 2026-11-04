import { useTranslation } from 'react-i18next';

import type { OverviewResponse } from '@/api/apm';
import { Readout } from '@/shell/readout/Readout';

import { throughputOf } from '../catalog/model';
import { formatCount, formatDuration, formatRate, formatThroughput } from '../format';
import { apdexTone, errorRateTone } from './model';

const CELL = 'bg-[var(--functional-surface)] px-[20px] py-[16px]';

/** The Apdex threshold the backend reads with until a service sets its own. */
const APDEX_THRESHOLD_MICROS = 500_000;

/**
 * The opening statement of the APM overview: how many services served, how
 * much, how often they failed, how long they took and how satisfied their
 * callers were, all services together. One surface of figures; how they
 * moved is in the charts under it.
 */
export function OverviewPulse({ data, range }: { data: OverviewResponse; range: { from: Date; to: Date } }) {
  const { t } = useTranslation('apm');
  const { red } = data;
  const services = data.serviceCount ?? data.services.length;
  const active = data.activeServiceCount ?? services;
  const percentiles = [
    ['P50', red.p50Time],
    ['P90', red.p90Time],
    ['P99', red.p99Time],
  ] as const;

  return (
    <section
      aria-label={t('landing.pulse_aria')}
      data-apm-overview-pulse=""
      className="ms-rise grid gap-px overflow-hidden rounded-md bg-bd-0 [box-shadow:var(--shadow-functional-surface)] sm:grid-cols-2 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.8fr)_minmax(0,1fr)]"
    >
      <Readout
        className={CELL}
        label={t('landing.kpi.services')}
        value={formatCount(active)}
        detail={t('landing.kpi.services_detail', { count: services, idle: services - active })}
      />
      <Readout
        className={CELL}
        riseIndex={1}
        label={t('landing.kpi.throughput')}
        value={formatThroughput(throughputOf(red.requestCount, range))}
        detail={t('landing.kpi.requests', { count: red.requestCount, formatted: formatCount(red.requestCount) })}
      />
      <Readout
        className={CELL}
        riseIndex={2}
        label={t('landing.kpi.error_rate')}
        value={formatRate(red.errorRate)}
        tone={errorRateTone(red.errorRate, red.requestCount)}
        detail={t('landing.kpi.errors', { count: red.errorCount, formatted: formatCount(red.errorCount) })}
      />
      <Readout
        className={CELL}
        riseIndex={3}
        label={t('landing.kpi.p95')}
        value={formatDuration(red.p95Time)}
        detail={[
          ...percentiles.map(([label, micros]) => `${label} ${formatDuration(micros ?? undefined)}`),
          t('landing.kpi.average', { value: formatDuration(red.durationAverage) }),
        ].join(' · ')}
      />
      <Readout
        className={CELL}
        riseIndex={4}
        label={t('landing.kpi.apdex')}
        value={red.apdex === null || red.apdex === undefined ? '—' : red.apdex.toFixed(2)}
        tone={apdexTone(red.apdex)}
        detail={t('landing.kpi.apdex_detail', { threshold: formatDuration(APDEX_THRESHOLD_MICROS) })}
      />
    </section>
  );
}

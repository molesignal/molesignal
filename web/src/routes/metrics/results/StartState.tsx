import { ArrowRight, Search } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import { resolveMetricType } from '@/lib/metricTypes';
import { PulseLine } from '@/shell/PulseLine';
import { Skeleton } from '@/shell/ui/skeleton';

import { MetricTypePrefix } from '../MetricTypePrefix';
import type { MetricsExploreResultsProps } from './types';

/** Two rows of starters at the widest layout; more than that is the browser's job. */
const STARTER_LIMIT = 8;

/**
 * What the graph area shows before the first query: the prompt, and the
 * metrics this organization already has, each one click away from a chart.
 *
 * An empty plot with a sentence in it leaves the reader to recall a metric
 * name; the catalog is already loaded, so the first chart can be picked here.
 */
export function MetricsStartState({
  starter,
}: {
  starter: MetricsExploreResultsProps['starter'];
}) {
  const { t } = useTranslation('metrics');
  const metrics = starter.metrics.slice(0, STARTER_LIMIT);

  return (
    <div
      data-testid="metrics-start"
      className="flex min-h-[320px] flex-[1_0_auto] flex-col items-center justify-center gap-6 px-4 py-8"
    >
      <div role="status" className="flex flex-col items-center gap-3 text-center">
        <PulseLine className="h-10 w-32 text-tx-3" />
        <p className="max-w-xl text-pretty font-sans text-sm leading-relaxed text-tx-2">
          {t('explore.chart.select_prompt')}
        </p>
      </div>

      {starter.pending ? (
        <StarterGrid>
          {Array.from({ length: 4 }, (_, index) => (
            <Skeleton key={index} className="h-[52px] rounded-md" />
          ))}
        </StarterGrid>
      ) : metrics.length > 0 ? (
        <section
          aria-labelledby="metrics-start-heading"
          className="w-full max-w-[1040px]"
        >
          <div className="mb-2 flex items-center justify-between gap-3 px-1">
            <h2 id="metrics-start-heading" className="type-eyebrow font-sans text-tx-2">
              {t('explore.start.heading')}
            </h2>
            <button
              type="button"
              onClick={starter.onBrowse}
              className="inline-flex h-7 items-center gap-1 rounded px-2 font-sans text-xs font-strong text-indigo-soft transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
            >
              <Search className="h-3 w-3" aria-hidden="true" />
              {t('explore.start.browse')}
            </button>
          </div>
          <StarterGrid>
            {metrics.map((metric) => (
              <StarterMetric key={metric.name} metric={metric} onPick={starter.onPick} />
            ))}
          </StarterGrid>
        </section>
      ) : (
        <div className="flex flex-col items-center gap-3 text-center">
          <p className="font-sans text-xs text-tx-3">{t('explore.start.no_metrics')}</p>
          <Link
            to="/datasource/applications/opentelemetry?signal=metrics"
            className="inline-flex h-9 items-center gap-2 rounded-md bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
          >
            {t('explore.start.connect')}
            <ArrowRight className="h-3.5 w-3.5" aria-hidden="true" />
          </Link>
        </div>
      )}
    </div>
  );
}

function StarterGrid({ children }: { children: React.ReactNode }) {
  return (
    <div className="grid w-full max-w-[1040px] grid-cols-[repeat(auto-fill,minmax(232px,1fr))] gap-[8px]">
      {children}
    </div>
  );
}

function StarterMetric({
  metric,
  onPick,
}: {
  metric: MetricCatalogEntry;
  onPick: (metric: MetricCatalogEntry) => void;
}) {
  const { t } = useTranslation('metrics');
  const type = resolveMetricType(metric);

  return (
    <button
      type="button"
      onClick={() => onPick(metric)}
      aria-label={t('explore.start.metric_aria', { metric: metric.name })}
      className="group flex min-h-[52px] min-w-0 items-center gap-2 rounded-md bg-[var(--control-surface)] px-3 py-2 text-left transition-colors hover:bg-bg-3 focus-visible:bg-bg-3"
    >
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-baseline gap-2 font-mono text-xs">
          <MetricTypePrefix type={type} label={t(`explore.types.${type}`)} />
          <span className="truncate font-semibold text-tx-0">{metric.name}</span>
        </span>
        <span className="mt-0.5 block truncate pl-[38px] font-sans text-xs text-tx-3">
          {t('explore.catalog.metric_meta', {
            labels: metric.labels.length,
            fields: metric.field_count,
          })}
        </span>
      </span>
      <ArrowRight
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 -translate-x-1 text-tx-3 opacity-0 transition-[opacity,transform] duration-fast group-hover:translate-x-0 group-hover:opacity-100 group-focus-visible:translate-x-0 group-focus-visible:opacity-100"
      />
    </button>
  );
}

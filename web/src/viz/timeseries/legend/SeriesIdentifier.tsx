import { ChevronRight } from 'lucide-react';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import type { TimeSeriesLegendStat, TimeSeriesSeries } from '../types';

export interface SeriesIdentityConfig {
  title: string;
  countLabel: string;
  nameLabel: string;
  labelCountLabel: (count: number) => string;
  expandLabel: (metricName: string) => string;
  collapseLabel: (metricName: string) => string;
  statLabels?: Partial<Record<TimeSeriesLegendStat, string>>;
}

interface SeriesIdentifierProps {
  series: TimeSeriesSeries;
  hidden: boolean;
  expanded: boolean;
  text: SeriesIdentityConfig;
  onSelect: (event: React.MouseEvent<HTMLButtonElement>) => void;
  onExpandedChange: (expanded: boolean) => void;
  onFocusChange: (focused: boolean) => void;
}

/**
 * Compact series identity for exploratory views. The metric name stays visible;
 * the raw Prometheus/OpenTelemetry label set is disclosed only on demand.
 */
export function SeriesIdentifier({
  series,
  hidden,
  expanded,
  text,
  onSelect,
  onExpandedChange,
  onFocusChange,
}: SeriesIdentifierProps) {
  const metricName = resolveSeriesMetricName(series);
  const labels = seriesLabelEntries(series.labels);

  return (
    <div className="min-w-0 py-1">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <button
          type="button"
          aria-pressed={!hidden}
          aria-label={metricName}
          title={series.name}
          onClick={onSelect}
          onFocus={() => onFocusChange(true)}
          onBlur={() => onFocusChange(false)}
          className={cn(
            'min-w-0 truncate bg-transparent p-0 text-left font-sans text-[13px] font-medium',
            'focus-visible:bg-bg-2 focus-visible:text-tx-0',
            hidden ? 'text-tx-4' : 'text-tx-0',
          )}
        >
          {metricName}
        </button>
        {labels.length > 0 ? (
          <button
            type="button"
            aria-expanded={expanded}
            aria-label={expanded
              ? text.collapseLabel(metricName)
              : text.expandLabel(metricName)}
            onClick={() => onExpandedChange(!expanded)}
            className="inline-flex items-center gap-1 rounded px-1 py-0.5 font-sans text-xs text-tx-3 hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-0"
          >
            <ChevronRight
              aria-hidden="true"
              className={cn(
                'h-3 w-3 transition-transform',
                expanded && 'rotate-90',
              )}
            />
            {text.labelCountLabel(labels.length)}
          </button>
        ) : (
          <span className="font-sans text-xs text-tx-3">
            {text.labelCountLabel(0)}
          </span>
        )}
      </div>

      {expanded && labels.length > 0 ? (
        <dl
          aria-label={text.expandLabel(metricName)}
          className="mt-2 grid min-w-0 grid-cols-[minmax(7.5rem,auto)_minmax(0,1fr)] gap-x-3 gap-y-1 border-l border-bd-0 pl-3"
          data-testid="series-identifier-labels"
        >
          {labels.map(([key, value]) => (
            <React.Fragment key={key}>
              <dt
                className="truncate font-sans text-xs text-tx-3"
                title={key}
              >
                {key}
              </dt>
              <dd
                className="m-0 truncate font-sans text-xs text-tx-1"
                title={value}
              >
                {value}
              </dd>
            </React.Fragment>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

export function resolveSeriesMetricName(series: TimeSeriesSeries): string {
  const explicit = series.metricName?.trim();
  if (explicit) return explicit;
  const namePrefix = series.name.match(/^([^{}]+)(?:\{|$)/)?.[1]?.trim();
  return namePrefix || series.name;
}

function seriesLabelEntries(
  labels: TimeSeriesSeries['labels'],
): Array<[string, string]> {
  return Object.entries(labels ?? {})
    .filter(([key]) => key !== '__name__')
    .sort(([left], [right]) => left.localeCompare(right));
}

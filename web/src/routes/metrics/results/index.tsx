import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { QueryRecommendations } from '@/shell/query/Recommendations';

import { GraphView } from './GraphView';
import { InspectorView } from './InspectorView';
import { TableView } from './TableView';
import type { MetricsExploreResultsProps } from './types';

type ResultsView = 'graph' | 'table' | 'inspector';

export function MetricsExploreResults({
  query,
  series,
  chart,
  exemplars,
  timeRangeSeconds,
  language,
  preferredView,
  onPreferredViewChange,
  onViewRawCounter,
  onInspectMetricType,
}: MetricsExploreResultsProps) {
  const { t } = useTranslation('metrics');
  const [activeView, setActiveView] = React.useState<ResultsView>(preferredView);

  React.useEffect(() => {
    setActiveView(preferredView);
  }, [preferredView]);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-auto bg-bg-0 px-3 pb-3 pt-2"
      data-testid="metrics-workspace"
    >
      <section className="flex min-h-[480px] flex-1 flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1">
        <div className="flex min-h-11 shrink-0 flex-wrap items-center border-b border-bd-0 bg-bg-2/50">
          <div className="flex min-w-0 flex-1 overflow-x-auto" role="tablist">
            {(
              [
                ['graph', t('explore.results.graph')],
                ['table', t('explore.results.table')],
                ['inspector', t('explore.results.inspector')],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={activeView === id}
                onClick={() => {
                  setActiveView(id);
                  if (id === 'graph' || id === 'table') {
                    onPreferredViewChange(id);
                  }
                }}
                className={`m-1 h-9 shrink-0 rounded-md px-3 font-sans text-xs font-semibold transition-colors focus-visible:bg-bg-3 ${
                  activeView === id
                    ? 'bg-bg-4 text-tx-0'
                    : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0'
                }`}
              >
                {label}
              </button>
            ))}
          </div>
          {query.result ? (
            <div className="flex shrink-0 items-center gap-3 px-3 font-sans text-xs text-tx-3">
              <span>
                {t('explore.results.series_count', {
                  count: series.metricSeries.length,
                })}
              </span>
              <span className="tabular-nums">
                {t('explore.results.query_time', {
                  ms: query.result.took_ms,
                })}
              </span>
            </div>
          ) : null}
        </div>

        {activeView === 'graph' ? (
          <GraphView
            query={query}
            series={series}
            chart={chart}
            exemplars={exemplars}
            onViewRawCounter={onViewRawCounter}
            onInspectMetricType={onInspectMetricType}
          />
        ) : activeView === 'table' ? (
          <TableView result={query.result} pending={query.pending} error={query.error} />
        ) : (
          <InspectorView
            result={query.result}
            statement={query.executedPromql ?? query.promql}
            pending={query.pending}
            error={query.error}
            metricSeriesCount={series.metricSeries.length}
            quality={series.quality}
            timeRangeSeconds={timeRangeSeconds}
            language={language}
            onViewRawCounter={onViewRawCounter}
            onInspectMetricType={onInspectMetricType}
            counterRateQuery={series.counterRateQuery}
          />
        )}
      </section>

      {query.result ? (
        <QueryRecommendations
          result={query.result}
          statement={query.executedPromql ?? ''}
          language="promql"
          timeRangeSecs={Math.round(timeRangeSeconds)}
          className="mt-3"
        />
      ) : null}
    </div>
  );
}

export type { MetricsExploreResultsProps } from './types';

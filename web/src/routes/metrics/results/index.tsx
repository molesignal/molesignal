import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { QueryRecommendations } from '@/shell/query/Recommendations';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

import { ChartControls } from './ChartControls';
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
  recovery,
  starter,
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

  const selectView = (key: string) => {
    const view = key as ResultsView;
    setActiveView(view);
    if (view === 'graph' || view === 'table') onPreferredViewChange(view);
  };
  const graph = activeView === 'graph';
  const hasQuery = Boolean(query.promql.trim());

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-auto bg-[var(--page-canvas)] pb-[20px] pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)]"
      data-testid="metrics-workspace"
    >
      <section
        className={cn(
          'flex flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
          // The graph takes whatever the page has left and grows past it with
          // its legend; a table or the inspector scrolls inside the card.
          graph ? 'flex-[1_0_auto]' : 'min-h-[480px] flex-1',
        )}
      >
        <div
          data-metrics-results-header
          className="flex min-h-[40px] shrink-0 flex-wrap items-center gap-x-3 border-b border-bd-0 pr-2"
        >
          <ModuleTabs
            variant="inline"
            label={t('explore.results.views_aria')}
            value={activeView}
            onValueChange={selectView}
            className="min-w-0 shrink-0"
            items={[
              { key: 'graph', label: t('explore.results.graph') },
              { key: 'table', label: t('explore.results.table') },
              { key: 'inspector', label: t('explore.results.inspector') },
            ]}
          />
          {graph && hasQuery && (
            // What the chart shows: its name, and the statement as it ran
            // (pinned filters are injected into it).
            <div className="flex min-w-0 flex-1 basis-[120px] items-baseline gap-2 font-sans text-xs">
              <span className="max-w-[60%] shrink-0 truncate font-strong text-tx-0">
                {query.chartTitle}
              </span>
              <code
                className="type-micro min-w-0 truncate font-code text-tx-3"
                title={query.executedPromql ?? query.promql}
              >
                {query.executedPromql ?? query.promql}
              </code>
            </div>
          )}
          <div className="ml-auto flex flex-wrap items-center justify-end gap-1.5 py-1">
            {query.result ? (
              <span className="flex shrink-0 items-center gap-3 px-1 font-sans text-xs text-tx-3">
                <span>
                  {t('explore.results.series_count', {
                    count: series.metricSeries.length,
                  })}
                </span>
                <span className="tabular-nums">
                  {t('explore.results.query_time', { ms: query.result.took_ms })}
                </span>
              </span>
            ) : null}
            {graph && (
              <ChartControls chart={chart} canStack={series.chartSeries.length > 1} />
            )}
          </div>
        </div>

        {graph ? (
          <GraphView
            query={query}
            series={series}
            chart={chart}
            exemplars={exemplars}
            recovery={recovery}
            starter={starter}
            onViewRawCounter={onViewRawCounter}
            onInspectMetricType={onInspectMetricType}
          />
        ) : activeView === 'table' ? (
          <TableView
            result={query.result}
            pending={query.pending}
            error={query.error}
            recovery={recovery}
          />
        ) : (
          <InspectorView
            result={query.result}
            statement={query.executedPromql ?? query.promql}
            pending={query.pending}
            error={query.error}
            recovery={recovery}
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
          className="mt-3 shrink-0"
        />
      ) : null}
    </div>
  );
}

export type { MetricsExploreResultsProps } from './types';

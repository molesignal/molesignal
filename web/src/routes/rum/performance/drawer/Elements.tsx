import { Crosshair } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatCount } from '../../overview/format';
import { clause, explorerPath, formatVital, gradeOf, type OverviewScope } from '../../overview/model';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { GRADE_TEXT } from '../ink';
import { ATTRIBUTIONS, type Element, type Metric } from '../model';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

/** A file as it reads best: the host and the path, without the scheme. */
const shown = (resource: string): string => resource.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');

/**
 * The elements that set the level in the views of a value: the one painted last, the one whose
 * interaction took longest, the ones that moved. Each has how many views it was in and the 75th
 * percentile of those; where an element is a file, the file is told beside it. A row opens the views
 * it was in, in the explorer.
 */
export function ElementsCard({
  query,
  elements,
  metric,
  scope,
  group,
  language,
}: {
  query: Read;
  elements: readonly Element[];
  metric: Metric;
  scope: OverviewScope;
  /** What the views of the drawer are narrowed to, as the explorer's query says it. */
  group: string;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const attribution = ATTRIBUTIONS[metric.key];
  const withResource = Boolean(attribution?.resource);
  const tracks = withResource ? 'minmax(0,1.2fr) 64px 64px minmax(0,1.4fr)' : 'minmax(0,1fr) 64px 64px';

  return (
    <OverviewCard
      icon={Crosshair}
      tone="text-[var(--chart-5)]"
      title={t(`performance.elements.title.${metric.key}`)}
      description={t('performance.elements.description')}
      testId="performance-elements"
      bodyClassName="overflow-x-auto"
    >
      <CardState query={query} empty={elements.length === 0} emptyTitle={t('performance.elements.empty')}>
        <div role="table" aria-label={t(`performance.elements.title.${metric.key}`)} data-rum-elements="" className="min-w-[440px]">
          <div
            role="row"
            className="grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
            style={{ gridTemplateColumns: tracks }}
          >
            <span role="columnheader">{t('performance.elements.columns.element')}</span>
            <span role="columnheader" className="text-right">{t('performance.table.views')}</span>
            <span role="columnheader" className="text-right">{t('overview.vitals.label', { name: t(`performance.metrics.${metric.key}`) })}</span>
            {withResource && <span role="columnheader">{t('performance.elements.columns.resource')}</span>}
          </div>
          {elements.map((element) => (
            <div
              key={`${element.selector}\u0000${element.resource ?? ''}`}
              role="row"
              data-rum-element={element.selector}
              className="relative grid h-8 items-center gap-3 border-b border-[var(--row-rule)] px-3 font-sans text-xs hover:bg-bg-2 focus-within:bg-bg-2"
              style={{ gridTemplateColumns: tracks }}
            >
              <span role="cell" className="min-w-0">
                <Link
                  to={explorerPath('views', scope, [group, clause(attribution?.selector ?? '', element.selector)])}
                  title={t('performance.elements.open', { selector: element.selector })}
                  className="block truncate font-mono text-xs text-tx-0 outline-none after:absolute after:inset-0 after:content-[''] hover:underline focus-visible:underline"
                >
                  {element.selector}
                </Link>
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-0">{formatCount(element.views, language)}</span>
              <span
                role="cell"
                className={cn('text-right font-strong tabular-nums', element.value !== null && GRADE_TEXT[gradeOf(metric, element.value)])}
              >
                {element.value === null ? '—' : formatVital(metric, element.value)}
              </span>
              {withResource && (
                <span role="cell" className="min-w-0 truncate text-tx-2" title={element.resource ?? undefined}>
                  {element.resource ? shown(element.resource) : <span className="text-tx-3">{t('performance.elements.no_resource')}</span>}
                </span>
              )}
            </div>
          ))}
        </div>
      </CardState>
    </OverviewCard>
  );
}

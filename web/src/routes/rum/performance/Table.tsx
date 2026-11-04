import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { GRADE_TEXT } from './ink';
import {
  type Direction,
  DIMENSIONS,
  type DimensionKey,
  goodShare,
  METRICS,
  type Metric,
  MIN_VIEWS,
  type Row,
  type SortKey,
  TABLE_ROWS,
} from './model';
import { BODY_ROW, CONTROL, HEAD_ROW, NoMatch, SearchField, SortHeading, useTypedSearch } from './tableParts';
import { regionName } from '../_helpers';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import type { RumAgentSubject } from '../agent/prompt';
import { formatCount, formatPercent } from '../overview/format';
import { formatVital, gradeOf } from '../overview/model';
import { CardState } from '../overview/OverviewCard';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const TRACKS = 'minmax(160px,1.6fr) minmax(64px,0.6fr) repeat(5,minmax(64px,0.7fr)) minmax(132px,1.1fr)';

/** What a value of a dimension is called: a country by its name in the language of the page, any other value as it is. */
export function valueLabel(dimension: DimensionKey, value: string | null, language: string, none: string): string {
  if (value === null) return none;

  return dimension === 'country' ? regionName(value, language) : value;
}

/**
 * The views told apart by a dimension, the most viewed first until a column is chosen: how many
 * views each had, the 75th percentile of every level in the colour of its grade, and the share of its
 * views that had the level in front good. A value with too few views to say anything is grey and
 * stays below the rest when a level orders the table. A row opens what its views are made of.
 */
export function PerformanceTable({
  query,
  rows,
  total,
  dimension,
  onDimension,
  metric,
  sort,
  direction,
  onSort,
  search,
  onSearch,
  open,
  onOpen,
  subject,
  language,
}: {
  query: Read;
  /** What is shown: narrowed by the words and in the order chosen. */
  rows: readonly Row[];
  /** How many values the read answered with, before the words narrowed them. */
  total: number;
  dimension: DimensionKey;
  onDimension: (key: DimensionKey) => void;
  metric: Metric;
  sort: SortKey;
  direction: Direction;
  onSort: (key: SortKey) => void;
  search: string;
  onSearch: (words: string) => void;
  /** The value open in the drawer. */
  open: string;
  onOpen: (value: string) => void;
  /** The application, environment and range the rows are of, for Mole Agent to be asked about a row. */
  subject: Omit<RumAgentSubject, 'focus'>;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const [draft, setDraft] = useTypedSearch(search, onSearch);
  const none = t('performance.table.none');

  const name = t(`performance.metrics.${metric.key}`);
  const heading = (key: SortKey, label: string, align: 'left' | 'right' = 'right') => (
    <SortHeading
      key={key}
      column={key}
      label={label}
      active={sort === key}
      lit={key === metric.key}
      direction={direction}
      align={align}
      onSort={() => onSort(key)}
    />
  );

  return (
    <section data-rum-overview-card="performance-table" className={cn(surfacePanelClass, 'min-w-0 overflow-hidden')}>
      <div data-rum-list-head="" className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-bd-0 px-3 py-2">
        <FittedSelect
          aria-label={t('performance.table.by')}
          data-rum-dimension=""
          value={dimension}
          onValueChange={(value) => onDimension(value as DimensionKey)}
          options={DIMENSIONS.map((entry) => ({ value: entry.key, label: t(`performance.dimensions.${entry.key}`) }))}
          maxWidth={220}
          prefix={t('performance.table.by')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <SearchField
          value={draft}
          onChange={setDraft}
          placeholder={t('performance.table.search')}
          label={t('performance.table.search_label')}
        />
        <span data-rum-table-count="" className="font-sans text-xs text-tx-3">
          {total >= TABLE_ROWS
            ? t('performance.table.most_viewed', { count: total })
            : t('performance.table.count', { count: total })}
          {total > 0 && ` · ${t('performance.table.few_note', { views: MIN_VIEWS })}`}
        </span>
      </div>
      <CardState query={query} empty={total === 0} emptyTitle={t('performance.table.empty')}>
        <div className="overflow-x-auto">
          <div role="table" aria-label={t('performance.table.aria', { dimension: t(`performance.dimensions.${dimension}`) })} data-rum-performance-table="" className="min-w-[860px]">
            <div role="row" className={HEAD_ROW} style={{ gridTemplateColumns: TRACKS }}>
              <span role="columnheader">{t(`performance.dimensions.${dimension}`)}</span>
              {heading('views', t('performance.table.views'))}
              {METRICS.map((each) => heading(each.key, t(`performance.metrics.${each.key}`)))}
              {heading('good', t('performance.table.good', { name }), 'left')}
            </div>
            {rows.map((row) => {
              const label = valueLabel(dimension, row.value, language, none);
              const few = row.views < MIN_VIEWS;
              const share = goodShare(row, metric.key);

              return (
                <div
                  key={row.value ?? '\u0000'}
                  role="row"
                  data-rum-row={row.value ?? ''}
                  data-rum-few={few ? '' : undefined}
                  title={few ? t('performance.table.few', { views: MIN_VIEWS }) : undefined}
                  className={cn('group', BODY_ROW, row.value !== null && row.value === open && 'bg-[var(--floating-item-selected)]', few && 'text-tx-3')}
                  style={{ gridTemplateColumns: TRACKS }}
                >
                  <span role="cell" className="flex min-w-0 items-center gap-1.5">
                    {row.value === null ? (
                      <span className="block truncate text-tx-3">{label}</span>
                    ) : (
                      <>
                        <button
                          type="button"
                          data-rum-open=""
                          onClick={() => onOpen(row.value as string)}
                          aria-label={t('performance.table.open', { value: label })}
                          className={cn(
                            "block min-w-0 truncate border-0 bg-transparent p-0 text-left font-sans text-xs font-strong outline-none after:absolute after:inset-0 after:content-['']",
                            few ? 'text-tx-2' : 'text-tx-0',
                          )}
                        >
                          {label}
                        </button>
                        {/* Beside the name, over the row's own target; shown under the pointer or the focus, and always on a touch screen. */}
                        <RumInvestigateButton
                          look="icon"
                          title={t('agent.investigate_row', { value: label })}
                          className="opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:opacity-100"
                          subject={{
                            ...subject,
                            focus: {
                              kind: 'slice',
                              dimension: t(`performance.dimensions.${dimension}`),
                              value: label,
                              ...(row.levels[metric.key] !== null && !few
                                ? { vital: metric.key, figure: formatVital(metric, row.levels[metric.key] as number) }
                                : {}),
                            },
                          }}
                        />
                      </>
                    )}
                  </span>
                  <span role="cell" className="text-right tabular-nums">{formatCount(row.views, language)}</span>
                  {METRICS.map((each) => {
                    const value = row.levels[each.key];

                    return value === null ? (
                      <span key={each.key} role="cell" className="text-right text-tx-4">—</span>
                    ) : (
                      <span
                        key={each.key}
                        role="cell"
                        data-rum-grade={few ? undefined : gradeOf(each, value)}
                        className={cn('text-right font-strong tabular-nums', !few && GRADE_TEXT[gradeOf(each, value)])}
                      >
                        {formatVital(each, value)}
                      </span>
                    );
                  })}
                  <span role="cell" className="flex min-w-0 items-center gap-2">
                    {share === null ? (
                      <span className="text-tx-4">—</span>
                    ) : (
                      <>
                        <span aria-hidden className="h-[6px] min-w-0 flex-1 rounded-[3px] bg-bg-3">
                          <span className="block h-full rounded-[3px] bg-[var(--green)] opacity-80" style={{ width: `${share * 100}%` }} />
                        </span>
                        <span className="w-9 shrink-0 text-right tabular-nums">{formatPercent(share)}</span>
                      </>
                    )}
                  </span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && <NoMatch>{t('performance.table.no_match')}</NoMatch>}
        </div>
      </CardState>
    </section>
  );
}

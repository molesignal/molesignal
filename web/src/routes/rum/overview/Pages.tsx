import { FileText } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatShare } from '../_helpers';
import { formatCount } from './format';
import {
  clause,
  explorerPath,
  formatVital,
  type Grade,
  gradeOf,
  type OverviewScope,
  type PageRow,
  pageField,
  readPages,
  VITALS,
  type Vital,
} from './model';
import { CardState, OverviewCard } from './OverviewCard';

const GRADE_TEXT: Record<Grade, string> = {
  good: 'text-[var(--green-ink)]',
  needs_improvement: 'text-[var(--yellow-ink)]',
  poor: 'text-[var(--red-ink)]',
};

const ROW = 'relative grid h-8 items-center gap-3 border-b border-[var(--row-rule)] px-3 font-sans text-xs hover:bg-bg-2 focus-within:bg-bg-2';

/**
 * The most viewed pages of a browser application, or the screens of an app: how many views each
 * had, how fast the browser painted, answered and held still (the 75th percentile of each vital,
 * graded in the colour of its word), and how many of its views met an error. A row opens the
 * explorer on the views of that page.
 */
export function PagesCard({
  query,
  scope,
  browser,
  language,
}: {
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };
  scope: OverviewScope;
  browser: boolean;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const rows = readPages(query.data);
  const platform = browser ? 'browser' : 'flutter';
  const field = pageField(platform);

  const tracks = browser
    ? 'minmax(160px,1.8fr) repeat(3,minmax(64px,1fr)) minmax(72px,1fr) minmax(72px,1fr)'
    : 'minmax(160px,1fr) minmax(72px,0.4fr) minmax(72px,0.4fr)';

  return (
    <OverviewCard
      icon={FileText}
      tone="text-[var(--chart-2)]"
      title={t(browser ? 'overview.pages.title' : 'overview.pages.screens')}
      description={t(browser ? 'overview.pages.description' : 'overview.pages.description_screens')}
      testId="pages"
      bodyClassName="overflow-x-auto"
    >
      <CardState query={query} empty={rows.length === 0} emptyTitle={t('overview.pages.empty')}>
        <div role="table" aria-label={t('overview.pages.table')} data-rum-pages="" className="min-w-[560px]">
          <div
            role="row"
            className="grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
            style={{ gridTemplateColumns: tracks }}
          >
            <span role="columnheader">{t(browser ? 'overview.pages.columns.page' : 'overview.pages.columns.screen')}</span>
            <span role="columnheader" className="text-right">{t('overview.pages.columns.views')}</span>
            {browser &&
              VITALS.map((vital) => (
                <span key={vital.key} role="columnheader" className="text-right">
                  {t('overview.vitals.label', { name: t(`overview.vitals.names.${vital.key}`) })}
                </span>
              ))}
            <span role="columnheader" className="text-right">{t('overview.pages.columns.errors')}</span>
          </div>
          {rows.map((row) => (
            <div key={row.page} role="row" data-rum-page={row.page} className={ROW} style={{ gridTemplateColumns: tracks }}>
              <span role="cell" className="min-w-0">
                <Link
                  to={explorerPath('views', scope, [clause(field, row.page)])}
                  title={t('overview.pages.open', { page: row.page })}
                  className="block truncate font-strong text-tx-0 outline-none after:absolute after:inset-0 after:content-['']"
                >
                  {row.page}
                </Link>
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-0">{formatCount(row.views, language)}</span>
              {browser && <VitalCells row={row} />}
              <span role="cell" className="text-right tabular-nums text-tx-0">
                {row.errorRate === null || row.errorRate === 0 ? <span className="text-tx-4">—</span> : formatShare(row.errorRate)}
              </span>
            </div>
          ))}
        </div>
      </CardState>
    </OverviewCard>
  );
}

function VitalCells({ row }: { row: PageRow }) {
  return (
    <>
      {VITALS.map((vital) => (
        <VitalCell key={vital.key} vital={vital} value={row[vital.key]} />
      ))}
    </>
  );
}

function VitalCell({ vital, value }: { vital: Vital; value: number | null }) {
  if (value === null) {
    return (
      <span role="cell" className="text-right text-tx-4">
        —
      </span>
    );
  }

  return (
    <span role="cell" data-rum-grade={gradeOf(vital, value)} className={cn('text-right font-strong tabular-nums', GRADE_TEXT[gradeOf(vital, value)])}>
      {formatVital(vital, value)}
    </span>
  );
}

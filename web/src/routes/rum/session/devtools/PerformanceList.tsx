import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { type ViewRow } from './model';
import { HEAD, ListScroller, NoRows, ROW, LIT } from './parts';
import { useKeepInSight } from './useFollow';
import { formatCount } from '../../overview/format';
import { formatVital, gradeOf, VITALS } from '../../overview/model';
import { GRADE_TEXT } from '../../performance/ink';

const [LCP, INP, CLS] = VITALS as [(typeof VITALS)[number], (typeof VITALS)[number], (typeof VITALS)[number]];

const TRACKS = 'minmax(0,1fr) 72px 72px 64px 72px 80px';

/** A level of a view in the colour of its grade; none where the browser sent none. */
function Level({ vital, value }: { vital: (typeof VITALS)[number]; value: number | null }) {
  if (value === null) return <span className="text-tx-4">—</span>;

  return <span className={cn('font-strong tabular-nums', GRADE_TEXT[gradeOf(vital, value)])}>{formatVital(vital, value)}</span>;
}

/**
 * How each page of the session did: a row for each view, with its three levels graded and how many
 * requests and long tasks it met. The view the replay is in is lit. A row opens the load of its view,
 * which is the waterfall of its requests.
 */
export function PerformanceList({
  rows,
  lit,
  language,
  empty,
  onOpen,
}: {
  rows: readonly ViewRow[];
  /** The index of the view the replay is in, or -1. */
  lit: number;
  language: string;
  empty: string;
  onOpen: (view: ViewRow) => void;
}) {
  const { t } = useTranslation('rum');
  const list = React.useRef<HTMLDivElement>(null);

  useKeepInSight(list, lit);

  if (rows.length === 0) return <NoRows>{empty}</NoRows>;

  return (
    <ListScroller ref={list} label={t('session_detail.tools.performance.aria')} minWidth={560}>
      <div role="row" data-list-head="" className={HEAD} style={{ gridTemplateColumns: TRACKS }}>
        <span role="columnheader">{t('session_detail.tools.performance.view')}</span>
        <span role="columnheader" className="text-right">
          {t('performance.metrics.lcp')}
        </span>
        <span role="columnheader" className="text-right">
          {t('performance.metrics.inp')}
        </span>
        <span role="columnheader" className="text-right">
          {t('performance.metrics.cls')}
        </span>
        <span role="columnheader" className="text-right">
          {t('session_detail.tools.performance.resources')}
        </span>
        <span role="columnheader" className="text-right">
          {t('session_detail.tools.performance.long_tasks')}
        </span>
      </div>
      {rows.map((row, index) => (
        <div
          key={row.id}
          role="row"
          data-rum-view={row.id}
          data-current={lit === index ? 'true' : undefined}
          className={cn(ROW, lit === index && LIT)}
          style={{ gridTemplateColumns: TRACKS }}
        >
          <span role="cell" className="min-w-0">
            <button
              type="button"
              onClick={() => onOpen(row)}
              aria-label={t('session_detail.tools.performance.open', { view: row.name })}
              className="block max-w-full truncate border-0 bg-transparent p-0 text-left font-sans text-xs font-strong text-tx-0 outline-none after:absolute after:inset-0 after:content-['']"
            >
              {row.name}
            </button>
          </span>
          <span role="cell" className="text-right">
            <Level vital={LCP} value={row.lcp} />
          </span>
          <span role="cell" className="text-right">
            <Level vital={INP} value={row.inp} />
          </span>
          <span role="cell" className="text-right">
            <Level vital={CLS} value={row.cls} />
          </span>
          <span role="cell" className="text-right tabular-nums text-tx-1">
            {formatCount(row.resources, language)}
          </span>
          <span role="cell" className={cn('text-right tabular-nums', row.longTasks > 0 ? 'text-tx-1' : 'text-tx-4')}>
            {row.longTasks > 0 ? formatCount(row.longTasks, language) : '—'}
          </span>
        </div>
      ))}
    </ListScroller>
  );
}

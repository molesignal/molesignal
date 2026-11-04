import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { type Statement } from './model';
import { formatDurationMs } from '../../_helpers';
import { formatCount, formatPercent } from '../../overview/format';
import { FIGURE } from '../Reading';

/** A count of long tasks with its noun: "1 long task", "9 long tasks", the figure set in the strong step. */
function Counted({ value, language }: { value: number; language: string }) {
  return (
    <Trans
      i18nKey="performance.long_tasks.statement.tasks"
      ns="rum"
      count={value}
      values={{ formatted: formatCount(value, language) }}
      components={{ n: FIGURE }}
    />
  );
}

/**
 * The opening statement of the view: how many long tasks the pages ran into, how many of the page views
 * met at least one, and how long they ran and held up the input at the 75th percentile.
 */
export function LongTasksStatement({ statement, share, language }: { statement: Statement; share: number | null; language: string }) {
  const { t } = useTranslation('rum');
  const timed = statement.p75 !== null && statement.blocking !== null;

  const key = timed
    ? share === null
      ? 'performance.long_tasks.statement.sentence_alone'
      : 'performance.long_tasks.statement.sentence'
    : share === null
      ? 'performance.long_tasks.statement.sentence_count'
      : 'performance.long_tasks.statement.sentence_share';

  return (
    <section aria-label={t('performance.long_tasks.statement.aria')} data-rum-performance-statement="" className={cn(surfacePanelClass, 'px-5 py-4')}>
      <p data-rum-statement="" className="m-0 type-page-title font-sans font-strong text-tx-1">
        <Trans
          i18nKey={key}
          ns="rum"
          values={{
            share: share === null ? '' : formatPercent(share),
            duration: statement.p75 === null ? '' : formatDurationMs(statement.p75),
            blocking: statement.blocking === null ? '' : formatDurationMs(statement.blocking),
          }}
          components={{
            tasks: <Counted value={statement.tasks} language={language} />,
            n: FIGURE,
            share: <span className="font-display text-[var(--orange-ink)]" />,
          }}
        />
      </p>
    </section>
  );
}

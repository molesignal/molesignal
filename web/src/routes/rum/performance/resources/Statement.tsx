import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { failureRate, type Statement } from './model';
import { formatDurationMs, formatSize } from '../../_helpers';
import { formatCount } from '../../overview/format';
import { FIGURE } from '../Reading';
import { failureInk, formatRate } from '../tableParts';

/** A count of requests with its noun: "1 request", "9 requests", the figure set in the strong step. */
function Counted({ value, language }: { value: number; language: string }) {
  return (
    <Trans
      i18nKey="performance.resources.statement.requests"
      ns="rum"
      count={value}
      values={{ formatted: formatCount(value, language) }}
      components={{ n: FIGURE }}
    />
  );
}

/**
 * The opening statement of the view: how many requests the pages made, how long the 75th percentile of
 * them took, how many failed, and what a page view asked for on average.
 */
export function ResourcesStatement({ statement, language }: { statement: Statement; language: string }) {
  const { t } = useTranslation('rum');
  const rate = failureRate(statement);
  const duration = statement.p75 === null ? null : formatDurationMs(statement.p75);

  const key =
    duration !== null && rate !== null
      ? 'performance.resources.statement.sentence'
      : duration !== null
        ? 'performance.resources.statement.sentence_time'
        : 'performance.resources.statement.sentence_count';

  return (
    <section aria-label={t('performance.resources.statement.aria')} data-rum-performance-statement="" className={cn(surfacePanelClass, 'px-5 py-4')}>
      <p data-rum-statement="" className="m-0 type-page-title font-sans font-strong text-tx-1">
        <Trans
          i18nKey={key}
          ns="rum"
          values={{ duration, rate: rate === null ? '' : formatRate(rate) }}
          components={{
            requests: <Counted value={statement.requests} language={language} />,
            n: FIGURE,
            rate: <span data-rum-failure-rate="" className={cn('font-strong', failureInk(rate))} />,
          }}
        />
        {statement.views > 0 && (
          <>
            {' '}
            <Trans
              i18nKey="performance.resources.statement.per_view"
              ns="rum"
              values={{
                resources: formatCount(Math.round(statement.requests / statement.views), language),
                size: formatSize(statement.bytes / statement.views),
              }}
              components={{ n: FIGURE }}
            />
          </>
        )}
      </p>
    </section>
  );
}

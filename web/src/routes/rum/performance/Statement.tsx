import { Trans, useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';

import { GRADE_TEXT } from './ink';
import type { Level, MetricKey } from './model';
import { FIGURE, Reading } from './Reading';
import { formatCount } from '../overview/format';
import { formatVital } from '../overview/model';

/** A count of views with its noun: "1 page view", "9 page views", the figure set in the strong step. */
function Counted({ value, language }: { value: number; language: string }) {
  return (
    <Trans
      i18nKey="performance.statement.views"
      ns="rum"
      count={value}
      values={{ formatted: formatCount(value, language) }}
      components={{ n: FIGURE }}
    />
  );
}

/**
 * The opening statement of the page: how many views there were, and where the three vitals stand at
 * the 75th percentile, each in the colour of its grade. Under it the levels, to pick the one the
 * chart, the grades and the table are of: the three vitals, then the two that come before them.
 */
export function PerformanceStatement({
  views,
  levels,
  metric,
  onPick,
  language,
}: {
  views: number;
  /** In the order of the picker; the vitals are the first three. */
  levels: readonly Level[];
  metric: MetricKey;
  onPick: (metric: MetricKey) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const named = (key: MetricKey) => levels.find((level) => level.metric.key === key);
  const lcp = named('lcp');
  const inp = named('inp');
  const cls = named('cls');

  return (
    <section
      aria-label={t('performance.statement.aria')}
      data-rum-performance-statement=""
      className={cn(surfacePanelClass, 'space-y-4 px-5 py-4')}
    >
      <p data-rum-statement="" className="m-0 type-page-title font-sans font-strong text-tx-1">
        {lcp && inp && cls && (
          <Trans
            i18nKey="performance.statement.sentence"
            ns="rum"
            components={{
              views: <Counted value={views} language={language} />,
              lcp: <Reading level={lcp} />,
              inp: <Reading level={inp} />,
              cls: <Reading level={cls} />,
            }}
          />
        )}
      </p>
      <div role="group" aria-label={t('performance.picker.aria')} className="grid gap-1 [grid-template-columns:repeat(auto-fit,minmax(132px,1fr))]">
        {levels.map((level) => {
          const selected = level.metric.key === metric;
          const name = t(`performance.metrics.${level.metric.key}`);

          return (
            <button
              key={level.metric.key}
              type="button"
              aria-pressed={selected}
              data-rum-metric={level.metric.key}
              data-rum-grade={level.grade ?? 'none'}
              onClick={() => onPick(level.metric.key)}
              className={cn(
                'flex min-w-0 flex-col items-start gap-0.5 rounded-md border-0 px-3 py-2 text-left font-sans outline-none',
                selected ? 'bg-indigo-dim' : 'bg-transparent hover:bg-bg-2 focus-visible:bg-bg-3',
              )}
            >
              <span className={cn('type-caption font-strong', selected ? 'text-indigo-soft' : 'text-tx-2')}>
                {t('overview.vitals.label', { name })}
              </span>
              <span
                className={cn(
                  'type-page-title font-display tabular-nums',
                  level.grade ? GRADE_TEXT[level.grade] : 'text-tx-4',
                )}
              >
                {level.value === null ? '—' : formatVital(level.metric, level.value)}
              </span>
            </button>
          );
        })}
      </div>
    </section>
  );
}

import { Gauge } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { GRADE_FILL, GRADE_TEXT } from './ink';
import { type Level, type MetricKey, needsImprovement } from './model';
import { formatCount, formatPercent } from '../overview/format';
import { formatVital, type Grade } from '../overview/model';
import { CardState, OverviewCard } from '../overview/OverviewCard';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const GRADES: readonly Grade[] = ['good', 'needs_improvement', 'poor'];

const countOf = (level: Level, grade: Grade): number =>
  grade === 'good' ? level.good : grade === 'poor' ? level.poor : needsImprovement(level);

/**
 * How the views of the time in view divide among the three grades, for every level: a bar each, as
 * long as the views that had it, cut where they turned from good to needs improvement and on to
 * poor. The share that was good stands at its end; a row picks the level the page is of, and the
 * level in front says its three shares in words, with the line each is cut at.
 */
export function GradesCard({
  query,
  levels,
  metric,
  onPick,
  language,
}: {
  query: Read;
  levels: readonly Level[];
  metric: MetricKey;
  onPick: (metric: MetricKey) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const selected = levels.find((level) => level.metric.key === metric);

  return (
    <OverviewCard
      icon={Gauge}
      tone="text-[var(--chart-4)]"
      title={t('performance.grades.title')}
      description={t('performance.grades.description')}
      testId="performance-grades"
    >
      <CardState query={query} empty={levels.every((level) => level.samples === 0)} emptyTitle={t('performance.grades.empty')}>
        <ul className="m-0 list-none space-y-0.5 px-3 py-2">
          {levels.map((level) => {
            const name = t(`performance.metrics.${level.metric.key}`);
            const share = level.samples > 0 ? level.good / level.samples : null;

            return (
              <li key={level.metric.key}>
                <button
                  type="button"
                  aria-pressed={level.metric.key === metric}
                  data-rum-grades-row={level.metric.key}
                  onClick={() => onPick(level.metric.key)}
                  className={cn(
                    'grid h-[26px] w-full grid-cols-[44px_minmax(0,1fr)_40px] items-center gap-3 rounded-md border-0 bg-transparent px-1.5 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-3',
                    level.metric.key === metric && 'bg-bg-2',
                  )}
                >
                  <span className={cn('font-strong', level.metric.key === metric ? 'text-tx-0' : 'text-tx-2')}>{name}</span>
                  <span
                    role="img"
                    aria-label={t('performance.grades.aria', { name, good: formatPercent(share ?? 0) })}
                    className="flex h-[10px] gap-[2px]"
                  >
                    {level.samples === 0 ? (
                      <span className="flex-1 rounded-[3px] bg-bg-3" />
                    ) : (
                      GRADES.map((grade, index) => {
                        const views = countOf(level, grade);

                        return views === 0 ? null : (
                          <span
                            key={grade}
                            className={cn(
                              GRADE_FILL[grade],
                              'opacity-80',
                              index === 0 && 'rounded-l-[3px]',
                              index === GRADES.length - 1 && 'rounded-r-[3px]',
                            )}
                            style={{ flex: views }}
                          />
                        );
                      })
                    )}
                  </span>
                  <span className="text-right tabular-nums text-tx-1">{share === null ? '—' : formatPercent(share)}</span>
                </button>
              </li>
            );
          })}
        </ul>
        {selected && selected.samples > 0 && (
          <div data-rum-grades-detail={selected.metric.key} className="border-t border-bd-0 px-4 py-2.5 font-sans text-xs">
            <dl className="m-0 grid gap-x-3 gap-y-0.5 [grid-template-columns:auto_auto_auto_1fr]">
              {GRADES.map((grade) => (
                <GradeLine key={grade} level={selected} grade={grade} language={language} />
              ))}
            </dl>
            <p className="m-0 mt-1.5 text-tx-3">
              {t('performance.grades.note', {
                name: t(`performance.metrics.${selected.metric.key}`),
                formatted: formatCount(selected.samples, language),
              })}
            </p>
          </div>
        )}
      </CardState>
    </OverviewCard>
  );
}

/** A grade of the level in front: its word, the share of the views, how many, and the line it is cut at. */
function GradeLine({ level, grade, language }: { level: Level; grade: Grade; language: string }) {
  const { t } = useTranslation('rum');
  const views = countOf(level, grade);
  const { metric } = level;
  const range = {
    good: t('performance.grades.range.good', { good: formatVital(metric, metric.good) }),
    needs_improvement: t('performance.grades.range.needs_improvement', { good: formatVital(metric, metric.good), poor: formatVital(metric, metric.poor) }),
    poor: t('performance.grades.range.poor', { poor: formatVital(metric, metric.poor) }),
  }[grade];

  return (
    <>
      <dt className={cn('font-strong', GRADE_TEXT[grade])}>{t(`overview.vitals.grade.${grade}`)}</dt>
      <dd className="m-0 text-right tabular-nums text-tx-0">{formatPercent(level.samples > 0 ? views / level.samples : 0)}</dd>
      <dd className="m-0 text-right tabular-nums text-tx-1">{formatCount(views, language)}</dd>
      <dd className="m-0 text-tx-3">{range}</dd>
    </>
  );
}

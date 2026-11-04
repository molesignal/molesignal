import { useInfiniteQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as qualityApi from '@/api/agent/quality';
import type { EvaluatorName, ScoredAnswer } from '@/api/agent/quality';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';
import { Switch } from '@/shell/ui/switch';
import { formatRelativeMicros } from '@/time/relative';

import { AnswerDrawer } from './AnswerDrawer';
import { QualityFilters } from './Filters';
import { evaluatorName, overallScore } from './model';
import { ScoreWord } from './ScoreWord';
import { QUALITY_KEY, useQualityFilter, useScoringSettings } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

const HEADER = 'whitespace-nowrap';

/**
 * The scored answers, newest first: what was asked and answered, where it came
 * from, its score and who scored it. A row opens the answer with every score,
 * the facts behind them and its review.
 */
export function QualityAnswers() {
  const { t, i18n } = useTranslation('agent');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const filter = useQualityFilter();
  const { threshold } = useScoringSettings();
  const [lowOnly, setLowOnly] = React.useState(false);
  const [open, setOpen] = React.useState<ScoredAnswer | null>(null);

  const answers = useInfiniteQuery({
    queryKey: [...QUALITY_KEY, 'answers', filter.days, filter.source ?? 'all', lowOnly, threshold],
    queryFn: ({ pageParam }) =>
      qualityApi.listAnswers({
        days: filter.days,
        source: filter.source === 'personal_chat' ? undefined : filter.source,
        below: lowOnly ? threshold : undefined,
        cursor: pageParam,
      }),
    initialPageParam: null as qualityApi.Cursor | null,
    getNextPageParam: (page) => page.next,
    retry: false,
  });

  const rows = answers.data?.pages.flatMap((page) => page.answers) ?? [];
  const evaluators: EvaluatorName[] = answers.data?.pages[0]?.evaluators ?? [];
  const nowMicros = (answers.dataUpdatedAt || Date.now()) * 1000;

  const head = (
    <div className="flex flex-wrap items-center gap-3">
      <QualityFilters
        days={filter.days}
        source={filter.source}
        onDays={filter.setDays}
        onSource={filter.setSource}
        personal={false}
      />
      <label className="flex items-center gap-2 font-sans text-xs text-tx-1">
        <Switch checked={lowOnly} onCheckedChange={setLowOnly} aria-label={t('quality.answers.low_only')} />
        {t('quality.answers.low_only')}
      </label>
    </div>
  );

  return (
    <ModulePage
      title={t('quality.views.answers')}
      description={t('quality.answers.description')}
      action={head}
      flush
    >
      {answers.isPending ? (
        <ProductState variant="loading" />
      ) : answers.isError ? (
        <ProductState variant="error" error={answers.error} />
      ) : rows.length === 0 ? (
        <EmptyTab
          title={lowOnly ? t('quality.answers.none_low') : t('quality.empty_title')}
          hint={t('quality.answers.empty_hint')}
        />
      ) : (
        <div className="overflow-x-auto">
          <DataTable
            rows={rows}
            rowKey={(row) => `${row.targetKind}:${row.targetId}`}
            onRowClick={setOpen}
            emptyLabel={t('quality.empty_title')}
            className="min-w-[880px] rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'answer',
                header: t('quality.columns.answer'),
                className: 'w-full max-w-0',
                headerClassName: HEADER,
                cell: (row) => (
                  <div data-quality-answer={row.targetId} className="min-w-0 py-1">
                    <div className="truncate text-tx-0" title={row.question}>
                      {row.question || t('quality.answers.no_question')}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-tx-2" title={row.answer}>
                      {row.answer}
                    </div>
                  </div>
                ),
              },
              {
                key: 'source',
                header: t('quality.columns.source'),
                width: 120,
                headerClassName: HEADER,
                cell: (row) => <span className="text-tx-1">{t(`quality.sources.${row.source}`)}</span>,
              },
              {
                key: 'score',
                header: t('quality.columns.score'),
                width: 72,
                headerClassName: `${HEADER} text-right`,
                className: 'text-right',
                cell: (row) => <ScoreWord score={overallScore(row)} threshold={threshold} />,
              },
              {
                key: 'scorers',
                header: t('quality.columns.scored_by'),
                width: 168,
                headerClassName: HEADER,
                cell: (row) => <ScoredBy row={row} evaluators={evaluators} threshold={threshold} />,
              },
              {
                key: 'when',
                header: t('quality.columns.when'),
                width: 112,
                headerClassName: HEADER,
                cell: (row) => (
                  <span className="text-xs text-tx-2" title={formatMicrosActive(row.targetAt)}>
                    {formatRelativeMicros(row.targetAt, locale, nowMicros)}
                  </span>
                ),
              },
            ]}
          />
          {answers.hasNextPage && (
            <div className="flex justify-center py-3">
              <Button
                size="sm"
                variant="ghost"
                disabled={answers.isFetchingNextPage}
                onClick={() => void answers.fetchNextPage()}
              >
                {t('quality.answers.more')}
              </Button>
            </div>
          )}
        </div>
      )}
      <AnswerDrawer answer={open} threshold={threshold} onClose={() => setOpen(null)} />
    </ModulePage>
  );
}

/** Rules, and each evaluator that scored it, with their scores. */
function ScoredBy({
  row,
  evaluators,
  threshold,
}: {
  row: ScoredAnswer;
  evaluators: readonly EvaluatorName[];
  threshold: number;
}) {
  const { t } = useTranslation('agent');

  return (
    <div className="flex min-w-0 flex-col gap-0.5 text-xs">
      {row.scores.map((score) => (
        <div key={score.scorer} className="flex min-w-0 items-baseline gap-1.5">
          <span className="truncate text-tx-2">
            {score.scorer === 'rules' ? t('quality.rules') : evaluatorName(t, evaluators, score.scorer)}
          </span>
          <ScoreWord score={score.score} threshold={threshold} />
        </div>
      ))}
    </div>
  );
}

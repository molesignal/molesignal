import { useInfiniteQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as qualityApi from '@/api/agent/quality';
import type { ReviewItem, ReviewStatus } from '@/api/agent/quality';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { Button } from '@/shell/ui/button';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { formatRelativeMicros } from '@/time/relative';

import { AnswerDrawer } from './AnswerDrawer';
import { ReviewStatusWord } from './ReviewPanel';
import { ScoreWord } from './ScoreWord';
import { QUALITY_KEY, useScoringSettings } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

const HEADER = 'whitespace-nowrap';
const STATUSES: ReadonlyArray<ReviewStatus | 'all'> = ['pending', 'accepted', 'rejected', 'all'];

/**
 * Answers waiting for someone to look at them: scored below the threshold,
 * rated unhelpful, or drawn at random to keep the evaluators honest. An accepted
 * one joins the regression set the prompt check answers again.
 */
export function QualityReviews() {
  const { t, i18n } = useTranslation('agent');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const { threshold } = useScoringSettings();
  const [status, setStatus] = React.useState<ReviewStatus | 'all'>('pending');
  const [open, setOpen] = React.useState<ReviewItem | null>(null);

  const reviews = useInfiniteQuery({
    queryKey: [...QUALITY_KEY, 'reviews', status],
    queryFn: ({ pageParam }) => qualityApi.listReviews(status, pageParam),
    initialPageParam: null as qualityApi.Cursor | null,
    getNextPageParam: (page) => page.next,
    retry: false,
  });

  const rows = reviews.data?.pages.flatMap((page) => page.reviews) ?? [];
  const nowMicros = (reviews.dataUpdatedAt || Date.now()) * 1000;

  const select = (
    <FittedSelect
      aria-label={t('quality.reviews.status_label')}
      value={status}
      onValueChange={(value) => setStatus(value as ReviewStatus | 'all')}
      options={STATUSES.map((choice) => ({ value: choice, label: t(`quality.reviews.filter.${choice}`) }))}
      maxWidth={200}
    />
  );

  return (
    <ModulePage
      title={t('quality.views.reviews')}
      description={t('quality.reviews.description')}
      action={select}
      flush
    >
      {reviews.isPending ? (
        <ProductState variant="loading" />
      ) : reviews.isError ? (
        <ProductState variant="error" error={reviews.error} />
      ) : rows.length === 0 ? (
        <EmptyTab
          title={status === 'pending' ? t('quality.reviews.none_pending') : t('quality.reviews.none')}
          hint={t('quality.reviews.empty_hint')}
        />
      ) : (
        <div className="overflow-x-auto">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={setOpen}
            emptyLabel={t('quality.reviews.none')}
            className="min-w-[860px] rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'answer',
                header: t('quality.columns.answer'),
                className: 'w-full max-w-0',
                headerClassName: HEADER,
                cell: (row) => (
                  <div data-quality-review={row.id} className="min-w-0 py-1">
                    <div className="truncate text-tx-0" title={row.subject?.question}>
                      {row.subject?.question || t('quality.answers.no_question')}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-tx-2">
                      {t(`quality.sources.${row.source}`)}
                      {row.subject?.answer ? ` · ${row.subject.answer}` : ''}
                    </div>
                  </div>
                ),
              },
              {
                key: 'cause',
                header: t('quality.columns.cause'),
                width: 144,
                headerClassName: HEADER,
                cell: (row) => <span className="text-tx-1">{t(`quality.reviews.causes.${row.cause}`)}</span>,
              },
              {
                key: 'score',
                header: t('quality.columns.score'),
                width: 72,
                headerClassName: `${HEADER} text-right`,
                className: 'text-right',
                cell: (row) => <ScoreWord score={row.score} threshold={threshold} />,
              },
              {
                key: 'status',
                header: t('quality.columns.status'),
                width: 104,
                headerClassName: HEADER,
                cell: (row) => <ReviewStatusWord status={row.status} />,
              },
              {
                key: 'when',
                header: t('quality.columns.when'),
                width: 112,
                headerClassName: HEADER,
                cell: (row) => (
                  <span className="text-xs text-tx-2" title={formatMicrosActive(row.createdAt)}>
                    {formatRelativeMicros(row.createdAt, locale, nowMicros)}
                  </span>
                ),
              },
            ]}
          />
          {reviews.hasNextPage && (
            <div className="flex justify-center py-3">
              <Button
                size="sm"
                variant="ghost"
                disabled={reviews.isFetchingNextPage}
                onClick={() => void reviews.fetchNextPage()}
              >
                {t('quality.answers.more')}
              </Button>
            </div>
          )}
        </div>
      )}
      <AnswerDrawer
        answer={
          open
            ? { targetKind: open.targetKind, targetId: open.targetId, question: open.subject?.question ?? '' }
            : null
        }
        threshold={threshold}
        onClose={() => setOpen(null)}
      />
    </ModulePage>
  );
}

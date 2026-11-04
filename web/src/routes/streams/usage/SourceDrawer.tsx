import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as usageApi from '@/api/intakeUsage';
import type { SourceUsage, UsageBucket, UsageDetail, UsageDimension } from '@/api/intakeUsage';
import { useTimeFormatter } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer } from '@/shell/FormDrawer';
import { StatusWord } from '@/shell/StatusWord';

import {
  BUDGET_STATE_TONE,
  UNKNOWN_FIGURE,
  budgetFigure,
  bytesFigure,
  countFigure,
  formatBytes,
  shareFigure,
  sourceCaption,
  sourceName,
  usedFigure,
  type windowFromParam,
} from './model';

function HourlyBars({ buckets }: { buckets: readonly UsageBucket[] }) {
  const { t } = useTranslation('streams');
  const formatter = useTimeFormatter();
  const tallest = Math.max(0, ...buckets.map((bucket) => bucket.intakeBytes));

  return (
    <div
      role="img"
      aria-label={t('usage.detail.trend')}
      data-testid="usage-trend"
      className="flex h-24 items-end gap-px"
    >
      {buckets.map((bucket) => (
        <div
          key={bucket.startTime}
          title={`${formatter.micros(bucket.startTime, false)} · ${formatBytes(bucket.intakeBytes)}`}
          className="flex h-full min-w-0 flex-1 items-end"
        >
          <div
            className="w-full rounded-t-[2px] bg-indigo"
            style={{ height: tallest > 0 ? `${Math.max(2, (bucket.intakeBytes / tallest) * 100)}%` : 0 }}
          />
        </div>
      ))}
    </div>
  );
}

function Breakdown({ breakdown, total }: { breakdown: UsageDetail['breakdowns'][number]; total: number }) {
  const { t } = useTranslation('streams');

  return (
    <section data-testid={`usage-breakdown-${breakdown.dimension}`}>
      <h3 className="m-0 mb-2 font-sans text-sm font-medium text-tx-0">
        {t(`usage.detail.by_${breakdown.dimension}`)}
      </h3>
      {breakdown.sources.length === 0 ? (
        <p className="m-0 font-sans text-xs text-tx-3">{t('usage.detail.nothing')}</p>
      ) : (
        <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
          {breakdown.sources.map((row) => (
            <li key={row.sourceKey} className="flex items-baseline gap-3 font-sans text-sm">
              <span className="min-w-0 flex-1 truncate text-tx-1" title={sourceCaption(t, row.label)}>
                {sourceName(t, row.sourceKey, row.label)}
              </span>
              <span className="font-sans text-sm tabular-nums text-tx-1">{formatBytes(row.intakeBytes)}</span>
              <span className="w-14 text-right font-sans text-sm tabular-nums text-tx-3">
                {shareFigure(total > 0 ? row.intakeBytes / total : null)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** One source over the window: its hours, what it is made of, and where its budget stands. */
export function SourceDrawer({
  dimension,
  usageWindow,
  source,
  canManage,
  onClose,
  onSetBudget,
}: {
  dimension: UsageDimension;
  usageWindow: ReturnType<typeof windowFromParam>;
  source: SourceUsage | null;
  canManage: boolean;
  onClose: () => void;
  onSetBudget: (source: SourceUsage) => void;
}) {
  const { t } = useTranslation('streams');

  const query = useQuery({
    queryKey: ['intake-usage', 'detail', dimension, source?.sourceKey, usageWindow.key],
    queryFn: () =>
      usageApi.detail({
        dimension,
        sourceKey: source?.sourceKey ?? '',
        windowSecs: usageWindow.secs,
        bucketCount: usageWindow.buckets,
      }),
    enabled: source !== null,
  });

  const budget = source?.budget;

  return (
    <FormDrawer
      open={source !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={source ? sourceName(t, source.sourceKey, source.label) : ''}
      subtitle={source ? sourceCaption(t, source.label) : undefined}
      width={560}
    >
      {source && (
        <div className="flex flex-col gap-6">
          <section className="flex flex-wrap gap-x-8 gap-y-2">
            <div>
              <div className="font-sans text-xs text-tx-3">{t('usage.columns.intake_bytes')}</div>
              <div className="font-sans text-sm tabular-nums text-tx-0" data-testid="usage-detail-bytes">
                {bytesFigure(source.intakeBytes, source.complete)}
              </div>
            </div>
            <div>
              <div className="font-sans text-xs text-tx-3">{t('usage.columns.events')}</div>
              <div className="font-sans text-sm tabular-nums text-tx-0">{countFigure(source.events, source.complete)}</div>
            </div>
            <div>
              <div className="font-sans text-xs text-tx-3">{t('usage.columns.share')}</div>
              <div className="font-sans text-sm tabular-nums text-tx-0">{shareFigure(source.share)}</div>
            </div>
          </section>

          {query.isLoading ? (
            <ProductState variant="loading" compact />
          ) : query.isError || !query.data ? (
            <ProductState variant="error" error={query.error} compact />
          ) : (
            <>
              <section>
                <h3 className="m-0 mb-2 font-sans text-sm font-medium text-tx-0">{t('usage.detail.trend')}</h3>
                <HourlyBars buckets={query.data.buckets} />
              </section>
              {query.data.breakdowns.map((breakdown) => (
                <Breakdown key={breakdown.dimension} breakdown={breakdown} total={query.data.intakeBytes} />
              ))}
            </>
          )}

          {(budget || (canManage && source.budgetable)) && (
            <section data-testid="usage-detail-budget">
              <h3 className="m-0 mb-2 font-sans text-sm font-medium text-tx-0">{t('usage.columns.budget')}</h3>
              {budget ? (
                <p className="m-0 flex flex-wrap items-baseline gap-x-3 font-sans text-sm text-tx-1">
                  <span>{budgetFigure(t, budget)}</span>
                  <span>
                    {t('usage.detail.used', {
                      used: bytesFigure(budget.usedBytes, budget.usedComplete),
                      percent: usedFigure(budget),
                    })}
                  </span>
                  {budget.state ? (
                    <StatusWord tone={BUDGET_STATE_TONE[budget.state]}>{t(`usage.states.${budget.state}`)}</StatusWord>
                  ) : (
                    <span className="text-tx-3">{UNKNOWN_FIGURE}</span>
                  )}
                </p>
              ) : (
                <p className="m-0 font-sans text-sm text-tx-3">{t('usage.budget.none_hint')}</p>
              )}
              {canManage && (
                <ChromeButton size="sm" className="mt-2" onClick={() => onSetBudget(source)}>
                  {budget ? t('usage.budget.change') : t('usage.budget.set')}
                </ChromeButton>
              )}
            </section>
          )}
        </div>
      )}
    </FormDrawer>
  );
}

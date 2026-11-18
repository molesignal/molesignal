import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as intakeUsageApi from '@/api/intakeUsage';
import { canAccessProductPath, useProductAccess } from '@/product/access';
import { StatusWord } from '@/shell/StatusWord';
import { useAuthStore } from '@/stores/auth';

import { formatBytes } from '../canvas/format';
import { CanvasHeaderAction, CanvasSection } from '../canvas/layout';

const USAGE_PATH = '/streams/usage';
/** The budgets shown: the ones nearest their limit. */
const SHOWN_BUDGETS = 3;
/** From what share of its budget a source is drawn as close to it. */
const NEAR_SHARE = 0.8;

function secondsSinceMonthStart(now: Date): number {
  const start = new Date(now.getFullYear(), now.getMonth(), 1);
  return Math.max(60, Math.round((now.getTime() - start.getTime()) / 1000));
}

/** What the month took in, and the sources nearest their budget: for whoever reads intake usage. */
export function UsageCard({ className }: { className?: string }) {
  const { t } = useTranslation('onboarding');
  const nav = useNavigate();
  const access = useProductAccess();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const canRead = canAccessProductPath(USAGE_PATH, access);
  const [windowSecs] = React.useState(() => secondsSinceMonthStart(new Date()));
  const overview = useQuery({
    queryKey: ['streams', 'usage', 'workbench', orgId, windowSecs],
    queryFn: () => intakeUsageApi.overview({ dimension: 'stream', windowSecs }),
    enabled: canRead,
    staleTime: 5 * 60_000,
  });
  const budgets = useQuery({
    queryKey: ['streams', 'budgets', orgId],
    queryFn: intakeUsageApi.listBudgets,
    enabled: canRead,
    staleTime: 5 * 60_000,
  });

  if (!canRead) return null;

  const nearest = [...(budgets.data ?? [])]
    .filter((budget) => budget.usedRatio !== null)
    .sort((left, right) => (right.usedRatio ?? 0) - (left.usedRatio ?? 0))
    .slice(0, SHOWN_BUDGETS);
  const total = overview.data?.available ? overview.data.totalBytes : null;

  return (
    <CanvasSection
      title={t('workbench.usage.title')}
      className={className}
      actions={<CanvasHeaderAction label={t('workbench.usage.details')} onClick={() => nav(USAGE_PATH)} />}
      bodyClassName="px-[20px] pb-4"
      testId="workbench-usage"
    >
      <div className="flex items-baseline gap-2">
        <span className="font-sans text-[18px] font-display leading-[26px] tabular-nums text-tx-0">
          {total === null ? '—' : formatBytes(total)}
        </span>
        <span className="font-sans text-xs text-tx-3">{t('workbench.usage.taken_in')}</span>
      </div>
      {nearest.length > 0 ? (
        <>
          <div className="mt-3 font-sans text-xs text-tx-3">{t('workbench.usage.budgets')}</div>
          <ul className="mt-2 space-y-2">
            {nearest.map((budget) => {
              const ratio = budget.usedRatio ?? 0;
              const near = ratio >= NEAR_SHARE;
              return (
                <li key={budget.id} className="grid grid-cols-[minmax(0,1fr)_96px_40px] items-center gap-3">
                  <span className="truncate font-sans text-xs text-tx-1">{budget.label.name}</span>
                  <span className="relative block h-1.5 overflow-hidden rounded-[3px] bg-bg-3" aria-hidden="true">
                    <span
                      className="absolute inset-y-0 left-0 rounded-[3px]"
                      style={{
                        width: `${Math.min(100, ratio * 100)}%`,
                        background: near ? 'var(--orange-fill)' : 'var(--blue-fill)',
                      }}
                    />
                  </span>
                  <StatusWord tone={near ? 'orange' : 'dim'} strong={near} className="text-right text-xs tabular-nums">
                    {`${Math.round(ratio * 100)}%`}
                  </StatusWord>
                </li>
              );
            })}
          </ul>
        </>
      ) : (
        <p className="mt-2 font-sans text-xs text-tx-3">{t('workbench.usage.no_budgets')}</p>
      )}
    </CanvasSection>
  );
}

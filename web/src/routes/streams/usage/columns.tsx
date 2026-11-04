import type { TFunction } from 'i18next';

import type { DataTableColumn } from '@/admin';
import type { SourceUsage } from '@/api/intakeUsage';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { StatusWord } from '@/shell/StatusWord';

import {
  BUDGET_STATE_TONE,
  UNKNOWN_FIGURE,
  budgetFigure,
  budgetStateRank,
  bytesFigure,
  countFigure,
  shareFigure,
  sourceCaption,
  sourceName,
  usedFigure,
} from './model';

const FIGURE_CLASS = 'font-sans text-sm tabular-nums text-tx-1';

function Figure({ text }: { text: string }) {
  return <span className={cn(FIGURE_CLASS, text === UNKNOWN_FIGURE && 'text-tx-3')}>{text}</span>;
}

export function usageColumns({
  t,
  withBudgets,
  canManage,
  onSetBudget,
  onRemoveBudget,
}: {
  t: TFunction<'streams'>;
  /** The platform organization has no budgets, so nothing about them is shown there. */
  withBudgets: boolean;
  canManage: boolean;
  onSetBudget: (row: SourceUsage) => void;
  onRemoveBudget: (row: SourceUsage) => void;
}): DataTableColumn<SourceUsage>[] {
  const columns: DataTableColumn<SourceUsage>[] = [
    {
      key: 'source',
      header: t('usage.columns.source'),
      width: 300,
      sortable: true,
      value: (row) => sourceName(t, row.sourceKey, row.label),
      cell: (row) => {
        const caption = sourceCaption(t, row.label);

        return (
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="truncate font-semibold text-tx-0" title={sourceName(t, row.sourceKey, row.label)}>
                {sourceName(t, row.sourceKey, row.label)}
              </span>
              {row.label.missing && <StatusWord tone="dim">{t('usage.source.missing')}</StatusWord>}
            </div>
            {caption && <div className="mt-0.5 truncate text-xs text-tx-3">{caption}</div>}
          </div>
        );
      },
    },
    {
      key: 'source_key',
      header: t('usage.columns.source_key'),
      fileOnly: true,
      value: (row) => row.sourceKey,
      cell: (row) => row.sourceKey,
    },
    {
      key: 'intake_bytes',
      header: t('usage.columns.intake_bytes'),
      width: 116,
      sortable: true,
      value: (row) => row.intakeBytes,
      cell: (row) => <Figure text={bytesFigure(row.intakeBytes, row.complete)} />,
    },
    {
      key: 'events',
      header: t('usage.columns.events'),
      width: 112,
      sortable: true,
      value: (row) => row.events,
      cell: (row) => <Figure text={countFigure(row.events, row.complete)} />,
    },
    {
      key: 'share',
      header: t('usage.columns.share'),
      width: 84,
      sortable: true,
      value: (row) => row.share,
      cell: (row) => <Figure text={shareFigure(row.share)} />,
    },
  ];

  if (!withBudgets) return columns;

  return [
    ...columns,
    {
      key: 'budget',
      header: t('usage.columns.budget'),
      width: 132,
      sortable: true,
      value: (row) => (row.budget ? budgetFigure(t, row.budget) : null),
      sortValue: (row) => row.budget?.limitSize,
      cell: (row) =>
        row.budget ? (
          <span className={FIGURE_CLASS}>{budgetFigure(t, row.budget)}</span>
        ) : (
          <span className="text-xs text-tx-3">{t('usage.budget.none')}</span>
        ),
    },
    {
      key: 'used',
      header: t('usage.columns.used'),
      width: 116,
      sortable: true,
      value: (row) => row.budget?.usedBytes,
      cell: (row) => <Figure text={bytesFigure(row.budget?.usedBytes, row.budget?.usedComplete ?? true)} />,
    },
    {
      key: 'used_ratio',
      header: t('usage.columns.used_ratio'),
      width: 96,
      sortable: true,
      value: (row) => row.budget?.usedRatio,
      cell: (row) => <Figure text={usedFigure(row.budget)} />,
    },
    {
      key: 'state',
      header: t('usage.columns.state'),
      width: 104,
      sortable: true,
      value: (row) => (row.budget?.state ? t(`usage.states.${row.budget.state}`) : null),
      sortValue: (row) => budgetStateRank(row.budget?.state),
      cell: (row) =>
        row.budget?.state ? (
          <StatusWord tone={BUDGET_STATE_TONE[row.budget.state]} strong={row.budget.state === 'exceeded'}>
            {t(`usage.states.${row.budget.state}`)}
          </StatusWord>
        ) : (
          <Figure text={UNKNOWN_FIGURE} />
        ),
    },
    ...(canManage
      ? [
          {
            key: 'actions',
            header: '',
            width: 96,
            cell: (row: SourceUsage) =>
              row.budgetable ? (
                <ChromeButton size="sm" onClick={() => onSetBudget(row)}>
                  {row.budget ? t('usage.budget.change') : t('usage.budget.set')}
                </ChromeButton>
              ) : row.budget ? (
                // A source that is gone cannot take a new budget; the one it left can go too.
                <ChromeButton size="sm" onClick={() => onRemoveBudget(row)}>
                  {t('usage.budget.remove')}
                </ChromeButton>
              ) : null,
          },
        ]
      : []),
  ];
}

import { useTranslation } from 'react-i18next';

import type { Funnel, FunnelGroup } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatDurationMs } from '../_helpers';
import { formatCount, formatPercent } from '../overview/format';

/** The width of the column that carries a number in a row of the funnel. */
const NUMBER = 'w-[84px] shrink-0 text-right tabular-nums';

/**
 * The funnel of all the sessions: a row for each step with the sessions that got there as a bar as
 * long as their share of the first step's, the share they are of the first and of the step before,
 * and how long the sessions took to get there from the step before.
 */
export function FunnelBars({ group, language }: { group: FunnelGroup; language: string }) {
  const { t } = useTranslation('rum');
  const widest = Math.max(1, group.steps[0]?.sessions ?? 0);

  return (
    <div data-rum-funnel="" className="px-3 py-2">
      <div className="flex items-center gap-3 border-b border-bd-0 pb-1.5 font-sans text-xs text-tx-3">
        <span className="min-w-0 flex-1">{t('analytics.funnel.columns.step')}</span>
        <span className={NUMBER}>{t('analytics.funnel.columns.sessions')}</span>
        <span className={NUMBER}>{t('analytics.funnel.columns.conversion')}</span>
        <span className={cn(NUMBER, 'hidden sm:block')}>{t('analytics.funnel.columns.from_previous')}</span>
        <span className={cn(NUMBER, 'hidden md:block')}>{t('analytics.funnel.columns.median')}</span>
      </div>
      <ol className="m-0 list-none p-0">
        {group.steps.map((step, index) => {
          const dropped = step.fromPrevious !== null && index > 0 ? 1 - step.fromPrevious : null;

          return (
            <li
              key={`${index}:${step.name}`}
              data-rum-funnel-step={index}
              data-rum-funnel-sessions={step.sessions}
              className="flex items-center gap-3 border-b border-[var(--row-rule)] py-2 font-sans text-xs last:border-b-0"
            >
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2">
                  <span aria-hidden className="grid h-5 w-5 shrink-0 place-items-center rounded-full bg-bg-3 text-xs font-strong tabular-nums text-tx-1">
                    {index + 1}
                  </span>
                  <span className="truncate font-strong text-tx-0" title={step.name}>
                    {step.name}
                  </span>
                </span>
                <span aria-hidden className="mt-1.5 block h-[8px] rounded-[3px] bg-bg-3">
                  <span
                    className="block h-full rounded-[3px] bg-[var(--chart-1)] opacity-80"
                    style={{ width: `${Math.max(step.sessions > 0 ? 0.6 : 0, (step.sessions / widest) * 100)}%` }}
                  />
                </span>
              </span>
              <span className={cn(NUMBER, 'font-strong text-tx-0')}>{formatCount(step.sessions, language)}</span>
              <span className={cn(NUMBER, 'text-tx-1')}>{step.conversion === null ? '—' : formatPercent(step.conversion)}</span>
              <span className={cn(NUMBER, 'hidden sm:block', dropped !== null && dropped > 0.5 ? 'text-[var(--red-ink)]' : 'text-tx-2')}>
                {index === 0 || dropped === null ? '—' : dropped > 0 ? `−${formatPercent(dropped)}` : formatPercent(0)}
              </span>
              <span className={cn(NUMBER, 'hidden text-tx-2 md:block')}>{index === 0 || step.medianMs === null ? '—' : formatDurationMs(step.medianMs)}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** The same funnel for each group of sessions: what each got through, by the field they are counted by. */
export function FunnelGroups({ funnel, field, language }: { funnel: Funnel; field: string; language: string }) {
  const { t } = useTranslation('rum');
  const steps = funnel.groups[0]?.steps ?? [];

  return (
    <div className="overflow-x-auto">
      <table data-rum-funnel-groups="" className="w-full min-w-[520px] border-collapse font-sans text-xs">
        <thead>
          <tr className="border-b border-bd-0 text-tx-3">
            <th scope="col" className="px-3 py-1.5 text-left font-body first-letter:uppercase">
              {field}
            </th>
            {steps.map((step, index) => (
              <th key={`${index}:${step.name}`} scope="col" className="px-3 py-1.5 text-right font-body">
                <span className="block max-w-[140px] truncate" title={step.name}>
                  {index + 1} · {step.name}
                </span>
              </th>
            ))}
            <th scope="col" className="px-3 py-1.5 text-right font-body">
              {t('analytics.funnel.columns.overall')}
            </th>
          </tr>
        </thead>
        <tbody>
          {funnel.groups.map((group) => {
            const first = group.steps[0]?.sessions ?? 0;
            const last = group.steps[group.steps.length - 1]?.sessions ?? 0;

            return (
              <tr key={group.value ?? '\u0000'} data-rum-funnel-group={group.value ?? ''} className="border-b border-[var(--row-rule)] hover:bg-bg-2">
                <th scope="row" className="px-3 py-1.5 text-left font-strong text-tx-0">
                  {group.value ?? t('analytics.funnel.no_value')}
                </th>
                {group.steps.map((step, index) => (
                  <td key={`${index}:${step.name}`} className="px-3 py-1.5 text-right tabular-nums text-tx-1">
                    {formatCount(step.sessions, language)}
                    {index > 0 && step.conversion !== null && <span className="ml-1.5 text-tx-3">{formatPercent(step.conversion)}</span>}
                  </td>
                ))}
                <td className="px-3 py-1.5 text-right font-strong tabular-nums text-tx-0">{first > 0 ? formatPercent(last / first) : '—'}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

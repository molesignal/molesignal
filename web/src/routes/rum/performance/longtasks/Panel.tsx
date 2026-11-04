import { X } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import {
  type Group,
  groupFilters,
  invokersQuery,
  mainPage,
  pagesQuery,
  readSamples,
  readShares,
  type Row,
  sampleTasks,
  samplesQuery,
  samplesRowsQuery,
} from './model';
import { durationInk, scriptName } from './Table';
import { formatDurationMs } from '../../_helpers';
import { formatCount, formatPercent } from '../../overview/format';
import type { OverviewRange, OverviewScope } from '../../overview/model';
import { useDrawerAggregate, useViewRows } from '../drawer/reads';
import { PANEL_CLASS, Section, ShareBar } from '../PanelSection';
import { FIGURE } from '../Reading';

/**
 * What the long tasks of a group are made of: how they were triggered, the pages they happened on,
 * and the longest of them, each of which opens the load of the view it blocked, on that long task.
 */
export function LongTaskPanel({
  row,
  group,
  scope,
  range,
  search,
  language,
  onClose,
  onOpenView,
}: {
  row: Row;
  group: Group;
  scope: OverviewScope;
  range: OverviewRange;
  search: string;
  language: string;
  onClose: () => void;
  /** Opens the view that a long task blocked, on that long task. */
  onOpenView: (view: string, at: number | null, task: string) => void;
}) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();
  const filters = React.useMemo(() => groupFilters(group, row.values), [group, row.values]);

  const invokersRead = useDrawerAggregate('long-tasks-invokers', invokersQuery(scope, range, filters, search));
  const pagesRead = useDrawerAggregate('long-tasks-pages', pagesQuery(scope, range, filters, search));
  const samplesRead = useDrawerAggregate('long-tasks-samples', samplesQuery(scope, range, filters, search));

  const samples = React.useMemo(() => readSamples(samplesRead.data), [samplesRead.data]);
  const sampleRows = useViewRows(samples.length > 0 ? samplesRowsQuery(scope, range, samples.map((sample) => sample.id)) : null);
  const longest = React.useMemo(() => sampleTasks(samples, sampleRows.data ?? []), [sampleRows.data, samples]);

  const invokers = React.useMemo(() => readShares(invokersRead.data), [invokersRead.data]);
  const pages = React.useMemo(() => readShares(pagesRead.data), [pagesRead.data]);
  const page = mainPage(pages, row.tasks);

  const first = row.values[0] ?? null;
  const unattributed = group.key === 'script' && first === null;
  const title = unattributed ? t('performance.long_tasks.table.unattributed') : (first ?? t('performance.long_tasks.table.none'));
  const subtitle = [
    group.key === 'script' && row.values[1] ? row.values[1] : null,
    t('performance.long_tasks.panel.tasks', { count: row.tasks, formatted: formatCount(row.tasks, language) }),
  ]
    .filter(Boolean)
    .join(' · ');
  const shown =
    group.key === 'invoker' && first !== null
      ? t(`performance.long_tasks.invokers.${first}`, { defaultValue: first })
      : group.key === 'script' && first !== null
        ? scriptName(first)
        : title;

  return (
    <aside data-rum-panel="long-tasks" aria-label={t('performance.long_tasks.panel.aria')} className={PANEL_CLASS}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className={cn('truncate text-sm font-strong text-tx-0', group.key === 'script' && !unattributed && 'font-mono')} title={first ?? undefined}>
            {shown}
          </div>
          <div className="truncate text-xs text-tx-3">{subtitle}</div>
        </div>
        <button
          type="button"
          aria-label={t('performance.long_tasks.panel.close')}
          onClick={onClose}
          className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-0 bg-transparent p-0 text-tx-2 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
        </button>
      </div>

      {unattributed && <p className="m-0 text-xs leading-[19px] text-tx-3">{t('performance.long_tasks.panel.unattributed_note')}</p>}

      <p data-rum-panel-sentence="" className="m-0 text-sm leading-[21px] text-tx-1">
        {row.p75 !== null && row.blocking !== null ? (
          <Trans
            i18nKey="performance.long_tasks.panel.sentence"
            ns="rum"
            values={{ duration: formatDurationMs(row.p75), blocking: formatDurationMs(row.blocking) }}
            components={{ n: <span className={cn('font-display', durationInk(row.p75))} />, b: FIGURE }}
          />
        ) : row.p75 !== null ? (
          <Trans
            i18nKey="performance.long_tasks.panel.sentence_time"
            ns="rum"
            values={{ duration: formatDurationMs(row.p75) }}
            components={{ n: <span className={cn('font-display', durationInk(row.p75))} /> }}
          />
        ) : null}
        {page && (
          <>
            {' '}
            <Trans i18nKey="performance.long_tasks.panel.page" ns="rum" values={{ page }} components={{ code: <code className="font-mono text-tx-0" /> }} />
          </>
        )}
      </p>

      <Section
        title={t('performance.long_tasks.panel.invokers')}
        pending={invokersRead.isPending}
        failed={invokersRead.isError}
        unavailable={invokersRead.data?.unavailable}
        onRetry={() => void invokersRead.refetch()}
        empty={invokers.length === 0}
        emptyText={t('performance.long_tasks.panel.invokers_empty')}
        testId="invokers"
      >
        <div data-rum-invokers="" className="flex flex-col gap-1.5">
          {invokers.map((share) => (
            <div key={share.value ?? ''} data-rum-invoker={share.value ?? ''}>
              <ShareBar
                label={share.value === null ? t('performance.long_tasks.panel.unknown') : t(`performance.long_tasks.invokers.${share.value}`, { defaultValue: share.value })}
                labelWidth="96px"
                share={row.tasks > 0 ? share.tasks / row.tasks : 0}
                value={formatPercent(row.tasks > 0 ? share.tasks / row.tasks : 0)}
                fill="var(--blue)"
              />
            </div>
          ))}
        </div>
      </Section>

      {group.key !== 'page' && (
        <Section
          title={t('performance.long_tasks.panel.pages')}
          pending={pagesRead.isPending}
          failed={pagesRead.isError}
          unavailable={pagesRead.data?.unavailable}
          onRetry={() => void pagesRead.refetch()}
          empty={pages.length === 0}
          emptyText={t('performance.long_tasks.panel.pages_empty')}
          testId="pages"
        >
          <div data-rum-pages="" className="flex flex-col gap-1.5">
            {pages.map((share) => (
              <div key={share.value ?? ''} data-rum-page={share.value ?? ''}>
                <ShareBar
                  label={share.value ?? t('performance.long_tasks.panel.unknown')}
                  mono={share.value !== null}
                  labelWidth="96px"
                  share={row.tasks > 0 ? share.tasks / row.tasks : 0}
                  value={formatPercent(row.tasks > 0 ? share.tasks / row.tasks : 0)}
                  fill="var(--blue)"
                />
              </div>
            ))}
          </div>
        </Section>
      )}

      <Section
        title={t('performance.long_tasks.panel.samples')}
        pending={samplesRead.isPending || (samples.length > 0 && sampleRows.isPending)}
        failed={samplesRead.isError || sampleRows.isError}
        unavailable={samplesRead.data?.unavailable}
        onRetry={() => void (samplesRead.isError ? samplesRead.refetch() : sampleRows.refetch())}
        empty={longest.length === 0}
        emptyText={t('performance.long_tasks.panel.samples_empty')}
        testId="samples"
      >
        <ol data-rum-samples="" className="m-0 flex list-none flex-col p-0">
          {longest.map((task) => {
            const line = (
              <>
                <span className="min-w-0 flex-1 truncate text-tx-0">
                  {[task.at === null ? null : formatter.micros(task.at), task.page].filter(Boolean).join(' · ') || task.id}
                </span>
                <span className={cn('tabular-nums', durationInk(task.ms))}>{formatDurationMs(task.ms)}</span>
              </>
            );

            return (
              <li key={task.id} className="border-t border-bd-0 first:border-t-0">
                {task.view ? (
                  <button
                    type="button"
                    data-rum-sample={task.id}
                    aria-label={t('performance.long_tasks.panel.open_load', { page: task.page ?? task.id })}
                    onClick={() => onOpenView(task.view as string, task.at, task.id)}
                    className="flex w-full items-center gap-2 border-0 bg-transparent px-0 py-1.5 text-left text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-3"
                  >
                    {line}
                  </button>
                ) : (
                  <div className="flex items-center gap-2 py-1.5 text-xs">{line}</div>
                )}
              </li>
            );
          })}
        </ol>
      </Section>
    </aside>
  );
}

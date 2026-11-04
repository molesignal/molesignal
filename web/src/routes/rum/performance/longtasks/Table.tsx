import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { type Direction, GROUP_ROWS, type Group, GROUPS, type GroupKey, MIN_TASKS, type Row, SLOW_MS, type SortKey, VERY_SLOW_MS } from './model';
import { formatDurationMs } from '../../_helpers';
import { formatCount } from '../../overview/format';
import { CardState } from '../../overview/OverviewCard';
import { BODY_ROW, CONTROL, HEAD_ROW, NoMatch, SearchField, SortHeading, useTypedSearch } from '../tableParts';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const NUMBERS = 'repeat(4,minmax(64px,0.6fr))';

/** The ink a time is written in: a long task of a tenth of a second is worth a word, of a fifth of a second it is a fault. */
export function durationInk(ms: number | null): string {
  if (ms === null) return 'text-tx-4';
  if (ms >= VERY_SLOW_MS) return 'font-strong text-[var(--red-ink)]';
  if (ms >= SLOW_MS) return 'font-strong text-[var(--orange-ink)]';

  return 'text-tx-1';
}

/** The words a script is written in: its address without the scheme, which a page of one host does not need said. */
export const scriptName = (script: string): string => script.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');

const tracksOf = (group: Group): string => `${group.fields.length === 2 ? 'minmax(160px,1.6fr) minmax(100px,1fr)' : 'minmax(160px,1.6fr)'} ${NUMBERS}`;

/**
 * The groups of long tasks the pages ran into, the ones with the most first until a column is chosen:
 * how many there were, in how many views, and how long they ran and held up the input at the 75th
 * percentile. The long tasks of browsers that name no script are one group of their own, said as such.
 * A row opens what the long tasks of the group are made of.
 */
export function LongTasksTable({
  query,
  rows,
  group,
  onGroup,
  search,
  onSearch,
  sort,
  direction,
  onSort,
  open,
  onOpen,
  language,
}: {
  query: Read;
  rows: readonly Row[];
  group: Group;
  onGroup: (key: GroupKey) => void;
  search: string;
  onSearch: (words: string) => void;
  sort: SortKey;
  direction: Direction;
  onSort: (key: SortKey) => void;
  open: string;
  onOpen: (key: string) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const [draft, setDraft] = useTypedSearch(search, onSearch);
  const none = t('performance.long_tasks.table.none');
  const tracks = tracksOf(group);
  const scripts = group.key === 'script';

  const heading = (key: SortKey, label: string) => (
    <SortHeading key={key} column={key} label={label} active={sort === key} direction={direction} onSort={() => onSort(key)} />
  );

  const word = (field: string, value: string | null): string => {
    if (value === null) return none;
    if (field === 'long_task.top_script.invoker_type') return t(`performance.long_tasks.invokers.${value}`, { defaultValue: value });

    return field === 'long_task.top_script.source_url' ? scriptName(value) : value;
  };

  return (
    <section data-rum-overview-card="long-tasks-table" className={cn(surfacePanelClass, 'min-w-0 overflow-hidden')}>
      <div data-rum-list-head="" className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-bd-0 px-3 py-2">
        <FittedSelect
          aria-label={t('performance.long_tasks.table.by')}
          data-rum-group=""
          value={group.key}
          onValueChange={(value) => onGroup(value as GroupKey)}
          options={GROUPS.map((entry) => ({ value: entry.key, label: t(`performance.long_tasks.groups.${entry.key}`) }))}
          maxWidth={220}
          prefix={t('performance.long_tasks.table.by')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <SearchField
          value={draft}
          onChange={setDraft}
          placeholder={t('performance.long_tasks.table.search')}
          label={t('performance.long_tasks.table.search_label')}
        />
        <span data-rum-table-count="" className="font-sans text-xs text-tx-3">
          {rows.length >= GROUP_ROWS
            ? t('performance.long_tasks.table.most_tasks', { count: rows.length })
            : t('performance.long_tasks.table.count', { count: rows.length })}
          {rows.length > 0 && ` · ${t('performance.long_tasks.table.few_note', { tasks: MIN_TASKS })}`}
        </span>
      </div>
      <CardState query={query} empty={rows.length === 0 && search.trim() === ''} emptyTitle={t('performance.long_tasks.table.empty')}>
        <div className="overflow-x-auto">
          <div role="table" aria-label={t('performance.long_tasks.table.aria', { group: t(`performance.long_tasks.groups.${group.key}`) })} data-rum-long-tasks-table="" className="min-w-[700px]">
            <div role="row" className={HEAD_ROW} style={{ gridTemplateColumns: tracks }}>
              <span role="columnheader">{t(`performance.long_tasks.heads.${group.key}`)}</span>
              {scripts && <span role="columnheader">{t('performance.long_tasks.heads.function')}</span>}
              {heading('tasks', t('performance.long_tasks.table.tasks'))}
              {heading('views', t('performance.long_tasks.table.views'))}
              {heading('p75', t('performance.long_tasks.table.time'))}
              {heading('blocking', t('performance.long_tasks.table.blocking'))}
            </div>
            {rows.map((row) => {
              const few = row.tasks < MIN_TASKS;
              const first = row.values[0] ?? null;
              const unattributed = scripts && first === null;

              return (
                <div
                  key={row.key}
                  role="row"
                  data-rum-row={row.key}
                  data-rum-few={few ? '' : undefined}
                  data-rum-unattributed={unattributed ? '' : undefined}
                  title={few ? t('performance.long_tasks.table.few', { tasks: MIN_TASKS }) : undefined}
                  className={cn(BODY_ROW, row.key === open && 'bg-[var(--floating-item-selected)]', few && 'text-tx-3')}
                  style={{ gridTemplateColumns: tracks }}
                >
                  <span role="cell" className="min-w-0">
                    <button
                      type="button"
                      aria-pressed={row.key === open}
                      onClick={() => onOpen(row.key)}
                      aria-label={t('performance.long_tasks.table.open', { value: unattributed ? t('performance.long_tasks.table.unattributed') : word(group.fields[0] as string, first) })}
                      title={first ?? undefined}
                      className={cn(
                        "block max-w-full truncate border-0 bg-transparent p-0 text-left font-sans text-xs outline-none after:absolute after:inset-0 after:content-['']",
                        unattributed ? 'text-tx-3' : cn('font-strong', scripts ? 'font-mono' : '', few ? 'text-tx-2' : 'text-tx-0'),
                      )}
                    >
                      {unattributed ? t('performance.long_tasks.table.unattributed') : word(group.fields[0] as string, first)}
                    </button>
                  </span>
                  {scripts && (
                    <span role="cell" className={cn('min-w-0 truncate font-mono', row.values[1] ? 'text-tx-1' : 'text-tx-4')} title={row.values[1] ?? undefined}>
                      {row.values[1] ?? '—'}
                    </span>
                  )}
                  <span role="cell" className="text-right tabular-nums">{formatCount(row.tasks, language)}</span>
                  <span role="cell" className="text-right tabular-nums">{formatCount(row.views, language)}</span>
                  <span role="cell" className={cn('text-right tabular-nums', !few && durationInk(row.p75))}>
                    {row.p75 === null ? '—' : formatDurationMs(row.p75)}
                  </span>
                  <span role="cell" className={cn('text-right tabular-nums', !few && durationInk(row.blocking))}>
                    {row.blocking === null ? '—' : formatDurationMs(row.blocking)}
                  </span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && <NoMatch>{t('performance.long_tasks.table.no_match')}</NoMatch>}
        </div>
      </CardState>
    </section>
  );
}

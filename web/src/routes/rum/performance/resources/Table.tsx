import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { FittedSelect } from '@/shell/ui/fitted-select';

import {
  cacheShare,
  type Direction,
  failureRate,
  GROUP_ROWS,
  type Group,
  GROUPS,
  type GroupKey,
  MIN_REQUESTS,
  RESOURCE_TYPES,
  type Row,
  type SortKey,
  STATUS_FILTERS,
  type StatusFilter,
} from './model';
import { formatDurationMs, formatSize } from '../../_helpers';
import { formatCount, formatPercent } from '../../overview/format';
import { CardState } from '../../overview/OverviewCard';
import { typeInk } from '../../waterfall/ink';
import { BODY_ROW, CONTROL, failureInk, formatRate, HEAD_ROW, NoMatch, SearchField, SortHeading, useTypedSearch } from '../tableParts';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const NUMBERS = 'repeat(5,minmax(64px,0.7fr))';

/** What a column of the group is headed with, and what its values are written in. */
const HEADS: Record<string, string> = {
  'resource.url_host': 'host',
  'resource.type': 'type',
  'resource.url_path_group': 'path',
  'resource.protocol': 'protocol',
  'resource.delivery_type': 'delivery',
  'resource.render_blocking_status': 'blocking',
};

const tracksOf = (group: Group): string =>
  `${group.fields.length === 2 ? 'minmax(120px,1.4fr) minmax(72px,0.8fr)' : 'minmax(120px,1.6fr)'} ${NUMBERS}`;

/** How the value of a column of the group is written: the kind of request in its colour, a path as code, a host strong. */
function labelInk(field: string, value: string | null, index: number): string {
  if (value === null) return 'text-tx-3';
  if (field === 'resource.type') return cn('font-strong', typeInk(value));
  if (field === 'resource.url_path_group') return 'font-mono text-tx-0';

  return index === 0 ? 'font-strong text-tx-0' : 'text-tx-1';
}

/**
 * The groups of requests the pages made, the most requested first until a column is chosen: how many
 * there were, the 75th percentile of their time and size, the share that failed and the share
 * that came from the cache. A group with too few requests to say anything is grey and stays below the
 * rest when a column orders the table. A row opens what the requests of the group are made of.
 */
export function ResourcesTable({
  query,
  rows,
  group,
  onGroup,
  type,
  onType,
  status,
  onStatus,
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
  /** What is shown, in the order chosen. */
  rows: readonly Row[];
  group: Group;
  onGroup: (key: GroupKey) => void;
  type: string;
  onType: (type: string) => void;
  status: StatusFilter;
  onStatus: (status: StatusFilter) => void;
  search: string;
  onSearch: (words: string) => void;
  sort: SortKey;
  direction: Direction;
  onSort: (key: SortKey) => void;
  /** The group open in the panel. */
  open: string;
  onOpen: (key: string) => void;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const [draft, setDraft] = useTypedSearch(search, onSearch);
  const none = t('performance.resources.table.none');
  const tracks = tracksOf(group);

  const heading = (key: SortKey, label: string) => (
    <SortHeading key={key} column={key} label={label} active={sort === key} direction={direction} onSort={() => onSort(key)} />
  );

  return (
    <section data-rum-overview-card="resources-table" className={cn(surfacePanelClass, 'min-w-0 overflow-hidden')}>
      <div data-rum-list-head="" className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-bd-0 px-3 py-2">
        <FittedSelect
          aria-label={t('performance.resources.table.by')}
          data-rum-group=""
          value={group.key}
          onValueChange={(value) => onGroup(value as GroupKey)}
          options={GROUPS.map((entry) => ({ value: entry.key, label: t(`performance.resources.groups.${entry.key}`) }))}
          maxWidth={240}
          prefix={t('performance.resources.table.by')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <FittedSelect
          aria-label={t('performance.resources.table.type')}
          data-rum-type=""
          value={type === '' ? 'all' : type}
          onValueChange={(value) => onType(value === 'all' ? '' : value)}
          options={[{ value: 'all', label: t('performance.resources.table.all') }, ...RESOURCE_TYPES.map((entry) => ({ value: entry, label: entry }))]}
          maxWidth={200}
          prefix={t('performance.resources.table.type')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <FittedSelect
          aria-label={t('performance.resources.table.status')}
          data-rum-status=""
          value={status === '' ? 'all' : status}
          onValueChange={(value) => onStatus(value === 'all' ? '' : (value as StatusFilter))}
          options={[
            { value: 'all', label: t('performance.resources.table.all') },
            ...STATUS_FILTERS.map((entry) => ({ value: entry, label: t(`performance.resources.statuses.${entry}`) })),
          ]}
          maxWidth={240}
          prefix={t('performance.resources.table.status')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <SearchField
          value={draft}
          onChange={setDraft}
          placeholder={t('performance.resources.table.search')}
          label={t('performance.resources.table.search_label')}
        />
        <span data-rum-table-count="" className="font-sans text-xs text-tx-3">
          {rows.length >= GROUP_ROWS
            ? t('performance.resources.table.most_requested', { count: rows.length })
            : t('performance.resources.table.count', { count: rows.length })}
          {rows.length > 0 && ` · ${t('performance.resources.table.few_note', { requests: MIN_REQUESTS })}`}
        </span>
      </div>
      <CardState query={query} empty={rows.length === 0} emptyTitle={t('performance.resources.table.empty')}>
        <div className="overflow-x-auto">
          <div role="table" aria-label={t('performance.resources.table.aria', { group: t(`performance.resources.groups.${group.key}`) })} data-rum-resources-table="" className="min-w-[760px]">
            <div role="row" className={HEAD_ROW} style={{ gridTemplateColumns: tracks }}>
              {group.fields.map((field) => (
                <span key={field} role="columnheader">
                  {t(`performance.resources.heads.${HEADS[field] ?? 'host'}`)}
                </span>
              ))}
              {heading('requests', t('performance.resources.table.requests'))}
              {heading('p75', t('performance.resources.table.time'))}
              {heading('failed', t('performance.resources.table.failed'))}
              {heading('size', t('performance.resources.table.size'))}
              {heading('cache', t('performance.resources.table.cache'))}
            </div>
            {rows.map((row) => {
              const few = row.requests < MIN_REQUESTS;
              const rate = failureRate(row);
              const cache = cacheShare(row);

              return (
                <div
                  key={row.key}
                  role="row"
                  data-rum-row={row.key}
                  data-rum-few={few ? '' : undefined}
                  title={few ? t('performance.resources.table.few', { requests: MIN_REQUESTS }) : undefined}
                  className={cn(BODY_ROW, row.key === open && 'bg-[var(--floating-item-selected)]', few && 'text-tx-3')}
                  style={{ gridTemplateColumns: tracks }}
                >
                  {group.fields.map((field, index) => {
                    const value = row.values[index] ?? null;
                    const ink = cn(labelInk(field, value, index), few && value !== null && 'text-tx-2');

                    // The first label is the button that opens the group, and the row is its hit area.
                    return index === 0 ? (
                      <span key={field} role="cell" className="min-w-0">
                        <button
                          type="button"
                          aria-pressed={row.key === open}
                          onClick={() => onOpen(row.key)}
                          aria-label={t('performance.resources.table.open', { value: row.values.map((entry) => entry ?? none).join(' · ') })}
                          title={value ?? undefined}
                          className={cn(
                            "block max-w-full truncate border-0 bg-transparent p-0 text-left font-sans text-xs outline-none after:absolute after:inset-0 after:content-['']",
                            ink,
                          )}
                        >
                          {value ?? none}
                        </button>
                      </span>
                    ) : (
                      <span key={field} role="cell" title={value ?? undefined} className={cn('min-w-0 truncate', ink)}>
                        {value ?? none}
                      </span>
                    );
                  })}
                  <span role="cell" className="text-right tabular-nums">{formatCount(row.requests, language)}</span>
                  <span role="cell" className="text-right tabular-nums">{row.p75 === null ? <span className="text-tx-4">—</span> : formatDurationMs(row.p75)}</span>
                  <span role="cell" data-rum-failure={rate === null ? undefined : rate} className={cn('text-right tabular-nums', !few && failureInk(rate))}>
                    {rate === null ? '—' : formatRate(rate)}
                  </span>
                  <span role="cell" className="text-right tabular-nums">{row.size === null ? <span className="text-tx-4">—</span> : formatSize(row.size)}</span>
                  <span role="cell" className="text-right tabular-nums">{cache === null ? <span className="text-tx-4">—</span> : formatPercent(cache)}</span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && <NoMatch>{t('performance.resources.table.no_match')}</NoMatch>}
        </div>
      </CardState>
    </section>
  );
}

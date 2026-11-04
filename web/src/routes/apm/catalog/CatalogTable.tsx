import type { TFunction } from 'i18next';
import { ArrowDown, ArrowUp } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MetadataRules } from '@/api/apmSettings';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { formatRelativeMicros } from '@/time/relative';

import { gridMinWidth } from '../frame/gridWidth';
import { OpenRow, StarToggle } from '../frame/OpenRow';
import { describeHealth } from '../health';
import { serviceKey } from '../scope';
import { CatalogCell, type CatalogNames, tagInk } from './CatalogCell';
import type { CatalogGroup, CatalogGrouping } from './groups';
import { CATALOG_COLUMN_WIDTH, type CatalogColumn, type CatalogRow, type CatalogSort, type CatalogSortKey, isInactive, missingMetadata, NUMERIC_COLUMNS } from './model';

export { type CatalogNames, TIER_CLASS } from './CatalogCell';

const ROW_STYLE = { height: 32, ...longListRowStyle(31) };

const HEALTH_CLASS = {
  healthy: 'bg-[var(--green)]',
  warning: 'bg-[var(--yellow)]',
  critical: 'bg-red',
  no_traffic: 'bg-tx-4',
} as const;

/** The reader's starred services, and how a star is set or taken off. */
export interface CatalogFavorites {
  keys: ReadonlySet<string>;
  toggle: (service: { namespace: string; name: string }) => void;
}

/**
 * The services, a row each, as dense as the trace table: what each is and
 * whose, and how it has served in the time in view, in groups when the
 * catalog is grouped. A row opens the service; a service declared before it
 * reports opens its definition. The star keeps a service among the reader's
 * favorites, at the top of the list.
 */
export function CatalogTable({
  groups,
  grouping,
  columns,
  sort,
  onSortChange,
  range,
  names,
  favorites,
  asOfMicros,
  rules,
  onOpen,
}: {
  groups: readonly CatalogGroup[];
  grouping: CatalogGrouping;
  columns: readonly CatalogColumn[];
  sort: CatalogSort;
  onSortChange: (sort: CatalogSort) => void;
  range: { from: Date; to: Date };
  names: CatalogNames;
  favorites: CatalogFavorites;
  /** When a service that stopped reporting a day before counts as inactive: the end of the range. */
  asOfMicros: number;
  /** What the catalog asks of a service by its tier: one short of it says so. */
  rules: MetadataRules;
  onOpen: (row: CatalogRow) => void;
}) {
  const { t, i18n } = useTranslation('apm');
  const time = useTimeFormatter();
  const tracks = ['24px', 'minmax(220px,1fr)', ...columns.map((column) => CATALOG_COLUMN_WIDTH[column])];
  const grid = tracks.join(' ');
  // Head and rows are as wide as their columns: past the window the table scrolls as one.
  const minWidth = gridMinWidth(tracks);

  return (
    <div data-apm-catalog-table="" role="table" aria-label={t('catalog.table')} className="min-w-0 overflow-x-auto">
      <div
        role="row"
        className="sticky top-0 z-10 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
        style={{ gridTemplateColumns: grid, minWidth }}
      >
        {/* The star's column: named for screen readers, and holding its place in the grid. */}
        <span role="columnheader">
          <span className="sr-only">{t('catalog.columns.favorite')}</span>
        </span>
        <SortHeader column="service" sort={sort} onSortChange={onSortChange} t={t} />
        {columns.map((column) => (
          <SortHeader key={column} column={column} sort={sort} onSortChange={onSortChange} t={t} />
        ))}
      </div>
      {groups.map((group) => (
        <div key={group.key} role="rowgroup" data-apm-catalog-group={grouping ? group.key : undefined}>
          {grouping && <GroupHead group={group} grouping={grouping} names={names} minWidth={minWidth} t={t} />}
          {group.rows.map((row) => {
            const favorite = favorites.keys.has(serviceKey(row));
            // One word on where the service stands, the one that says most: an archived service
            // is expected to be quiet.
            const standing = row.definition?.archived ? 'archived' : !row.summary ? 'not_reporting' : isInactive(row, asOfMicros) ? 'inactive' : null;
            const missing = row.definition?.archived ? [] : missingMetadata(row, rules);

            return (
              <OpenRow
                key={row.key}
                data-apm-service-row={row.name}
                data-environment={row.environment ?? ''}
                onOpen={() => onOpen(row)}
                style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
                className={cn(
                  'grid w-full cursor-pointer items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs outline-none',
                  LONG_LIST_ROW_CLASS,
                  'hover:bg-bg-2 focus-visible:bg-bg-2',
                )}
              >
                <span role="cell" className="-ml-1 flex items-center">
                  <StarToggle
                    on={favorite}
                    label={t(favorite ? 'catalog.unfavorite' : 'catalog.favorite', { name: row.name })}
                    onToggle={() => favorites.toggle(row)}
                  />
                </span>
                <span role="cell" className="flex min-w-0 items-center gap-2">
                  <span
                    aria-hidden
                    data-apm-health={row.summary?.health}
                    title={row.summary ? describeHealth(row.summary.health, row.summary.healthReasons, t) : undefined}
                    className={cn('h-2 w-2 shrink-0 rounded-full', row.summary ? HEALTH_CLASS[row.summary.health] : 'bg-bd-1')}
                  />
                  <span className={cn('truncate font-strong', standing === 'archived' ? 'text-tx-2' : 'text-tx-0')} title={row.name}>
                    {row.name}
                  </span>
                  {/* Short of room, the namespace gives way first. */}
                  {row.namespace !== row.name && <span className="min-w-0 shrink-[8] truncate text-tx-3">{row.namespace}</span>}
                  {standing === 'archived' && (
                    <span data-apm-archived="" className="shrink-0 font-strong text-[var(--purple-ink)]">
                      {t('catalog.archived')}
                    </span>
                  )}
                  {standing === 'not_reporting' && <span className="shrink-0 text-tx-3">{t('catalog.not_reporting')}</span>}
                  {standing === 'inactive' && row.summary && (
                    <span
                      data-apm-inactive=""
                      className="shrink-0 text-tx-3"
                      title={t('service.facts.last_seen', { when: formatRelativeMicros(row.summary.lastSeenAt, i18n.language) })}
                    >
                      {t('catalog.inactive')}
                    </span>
                  )}
                  {missing.length > 0 && (
                    <span data-apm-missing-metadata={missing.join(',')} className="shrink-0 font-strong text-[var(--orange-ink)]" title={t('catalog.missing_hint')}>
                      {t('catalog.missing', { fields: missing.map((field) => t(`catalog.missing_fields.${field}`)).join(t('catalog.missing_and')) })}
                    </span>
                  )}
                </span>
                {columns.map((column) => (
                  <span key={column} role="cell" className={cn('flex min-w-0 items-center gap-2', NUMERIC_COLUMNS.has(column) && 'justify-end text-right')}>
                    <CatalogCell column={column} row={row} range={range} names={names} t={t} locale={i18n.language} absolute={time.micros} />
                  </span>
                ))}
              </OpenRow>
            );
          })}
        </div>
      ))}
    </div>
  );
}

/** A group's heading row: what its services share, and how many there are. */
function GroupHead({
  group,
  grouping,
  names,
  minWidth,
  t,
}: {
  group: CatalogGroup;
  grouping: CatalogGrouping;
  names: CatalogNames;
  minWidth: string;
  t: TFunction<'apm'>;
}) {
  const tag = grouping.startsWith('tag:') ? grouping.slice(4) : null;
  let label: React.ReactNode;

  if (!group.key) label = <span className="text-tx-3">{t(`catalog.group.none.${tag === null ? grouping : 'tag'}`, { tag })}</span>;
  else if (grouping === 'team') label = names.team(group.key);
  else if (grouping === 'tier') label = t(`definition.tier.${group.key}`);
  else if (tag !== null)
    label = (
      <>
        <span className={tagInk(tag)}>{tag}</span>
        <span className="text-tx-3">: </span>
        {group.key}
      </>
    );
  else label = group.key;

  return (
    <div role="row" data-apm-catalog-group-head="" className="flex h-8 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 font-sans text-xs" style={{ minWidth }}>
      <span role="cell" className="flex min-w-0 items-center gap-2">
        <span className="truncate font-strong text-tx-0">{label}</span>
        <span className="tabular-nums text-tx-3">{group.rows.length}</span>
      </span>
    </div>
  );
}

function SortHeader({
  column,
  sort,
  onSortChange,
  t,
}: {
  column: CatalogSortKey;
  sort: CatalogSort;
  onSortChange: (sort: CatalogSort) => void;
  t: TFunction<'apm'>;
}) {
  const label = t(`catalog.columns.${column}`);
  const active = sort.key === column;
  const numeric = column !== 'service' && NUMERIC_COLUMNS.has(column);
  const next: CatalogSort = active
    ? { key: column, direction: sort.direction === 'asc' ? 'desc' : 'asc' }
    : {
        key: column,
        direction: numeric || column === 'last_deploy' || column === 'last_seen' ? 'desc' : 'asc',
      };

  if (column === 'on_call') {
    return (
      <span role="columnheader" className="truncate">
        {label}
      </span>
    );
  }

  return (
    <span
      role="columnheader"
      aria-sort={active ? (sort.direction === 'asc' ? 'ascending' : 'descending') : undefined}
      className={cn('min-w-0', numeric && 'text-right')}
    >
      <button
        type="button"
        onClick={() => onSortChange(next)}
        className={cn(
          'relative inline-flex max-w-full items-center gap-1 rounded-sm after:absolute after:-inset-y-1 after:inset-x-0 after:content-[""] hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none',
          active && 'text-tx-0',
        )}
      >
        <span className="truncate">{label}</span>
        {active && (sort.direction === 'desc' ? <ArrowDown aria-hidden className="h-3 w-3 shrink-0" /> : <ArrowUp aria-hidden className="h-3 w-3 shrink-0" />)}
      </button>
    </span>
  );
}

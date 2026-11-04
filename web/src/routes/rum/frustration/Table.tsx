import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { type Direction, GROUP_ROWS, type Group, type GroupKey, groupWord, type Kind, type Row, type SortKey } from './model';
import { formatCount } from '../overview/format';
import { CardState } from '../overview/OverviewCard';
import { BODY_ROW, CONTROL, HEAD_ROW, NoMatch, SearchField, SortHeading, useTypedSearch } from '../performance/tableParts';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const tracksOf = (group: Group, kinds: number): string =>
  `${group.fields.length === 2 ? 'minmax(160px,1.4fr) minmax(120px,1.2fr)' : 'minmax(160px,1.6fr)'} repeat(${kinds + 1},minmax(64px,0.5fr))`;

/** The words a sign is called in, in a browser and in an app. */
export const kindKey = (kind: Kind, browser: boolean): string => `overview.frustration.kinds.${browser ? kind : `${kind}_tap`}`;

/**
 * The groups of frustrated actions, the ones that touched most sessions first until a column is chosen:
 * how many actions of each sign there were, and in how many sessions. An element with no name says so, and
 * keeps its selector beside the words. A row opens what its actions are made of.
 */
export function FrustrationTable({
  query,
  rows,
  group,
  groups,
  kinds,
  onGroup,
  kind,
  onKind,
  search,
  onSearch,
  sort,
  direction,
  onSort,
  open,
  onOpen,
  browser,
  language,
}: {
  query: Read;
  rows: readonly Row[];
  group: Group;
  groups: readonly Group[];
  /** The signs the application has: a column and a choice for each. */
  kinds: readonly Kind[];
  onGroup: (key: GroupKey) => void;
  kind: Kind | '';
  onKind: (kind: Kind | '') => void;
  search: string;
  onSearch: (words: string) => void;
  sort: SortKey;
  direction: Direction;
  onSort: (key: SortKey) => void;
  open: string;
  onOpen: (key: string) => void;
  browser: boolean;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const [draft, setDraft] = useTypedSearch(search, onSearch);
  const tracks = tracksOf(group, kinds.length);
  const elements = group.key === 'element';
  const unnamed = t('frustration.table.unnamed');
  const none = t('frustration.table.none');

  const heading = (key: SortKey, label: string) => (
    <SortHeading key={key} column={key} label={label} active={sort === key} direction={direction} onSort={() => onSort(key)} />
  );

  // The word of a row: its first value, or the words that say it has none; an element without a name keeps its selector beside them.
  const wordOf = (row: Row): { text: string; named: boolean } => {
    const first = row.values[0] ?? null;

    return first !== null ? { text: first, named: true } : { text: elements ? unnamed : none, named: false };
  };

  return (
    <section data-rum-overview-card="frustration-table" className={cn(surfacePanelClass, 'min-w-0 overflow-hidden')}>
      <div data-rum-list-head="" className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-bd-0 px-3 py-2">
        <FittedSelect
          aria-label={t('frustration.table.by')}
          data-rum-group=""
          value={group.key}
          onValueChange={(value) => onGroup(value as GroupKey)}
          options={groups.map((entry) => ({ value: entry.key, label: t(`frustration.groups.${groupWord(entry.key, browser)}`) }))}
          maxWidth={220}
          prefix={t('frustration.table.by')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <FittedSelect
          aria-label={t('frustration.table.kind')}
          data-rum-kind-filter=""
          value={kind === '' ? 'all' : kind}
          onValueChange={(value) => onKind(value === 'all' ? '' : (value as Kind))}
          options={[{ value: 'all', label: t('frustration.table.kinds_all') }, ...kinds.map((entry) => ({ value: entry, label: t(kindKey(entry, browser)) }))]}
          maxWidth={220}
          prefix={t('frustration.table.kind')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        <SearchField
          value={draft}
          onChange={setDraft}
          placeholder={t('frustration.table.search')}
          label={t('frustration.table.search_label')}
        />
        <span data-rum-table-count="" className="font-sans text-xs text-tx-3">
          {rows.length >= GROUP_ROWS ? t('frustration.table.most', { count: rows.length }) : t('frustration.table.count', { count: rows.length })}
        </span>
      </div>
      <CardState query={query} empty={rows.length === 0 && search.trim() === '' && kind === ''} emptyTitle={t('frustration.table.empty')}>
        <div className="overflow-x-auto">
          <div role="table" aria-label={t('frustration.table.aria', { group: t(`frustration.groups.${groupWord(group.key, browser)}`) })} data-rum-frustration-table="" className="min-w-[700px]">
            <div role="row" className={HEAD_ROW} style={{ gridTemplateColumns: tracks }}>
              <span role="columnheader">{t(`frustration.groups.${groupWord(group.key, browser)}`)}</span>
              {elements && browser && <span role="columnheader">{t('frustration.table.selector')}</span>}
              {kinds.map((entry) => heading(entry, t(kindKey(entry, browser))))}
              {heading('sessions', t('frustration.table.sessions'))}
            </div>
            {rows.map((row) => {
              const word = wordOf(row);
              const selector = row.values[1] ?? null;

              return (
                <div
                  key={row.key}
                  role="row"
                  data-rum-row={row.key}
                  data-rum-unnamed={elements && !word.named ? '' : undefined}
                  className={cn(BODY_ROW, row.key === open && 'bg-[var(--floating-item-selected)]')}
                  style={{ gridTemplateColumns: tracks }}
                >
                  <span role="cell" className="min-w-0">
                    <button
                      type="button"
                      aria-pressed={row.key === open}
                      onClick={() => onOpen(row.key)}
                      aria-label={t('frustration.table.open', { value: [word.text, !word.named && selector].filter(Boolean).join(' ') })}
                      title={word.text}
                      className={cn(
                        "block max-w-full truncate border-0 bg-transparent p-0 text-left font-sans text-xs outline-none after:absolute after:inset-0 after:content-['']",
                        word.named ? cn('font-strong text-tx-0', group.key === 'page' && 'font-mono') : 'text-tx-3',
                      )}
                    >
                      {word.text}
                    </button>
                  </span>
                  {elements && browser && (
                    <span role="cell" className={cn('min-w-0 truncate font-mono', selector ? 'text-tx-2' : 'text-tx-4')} title={selector ?? undefined}>
                      {selector ?? '—'}
                    </span>
                  )}
                  {kinds.map((entry) => (
                    <span key={entry} role="cell" data-rum-count={entry} className={cn('text-right tabular-nums', row[entry] === 0 ? 'text-tx-4' : 'text-tx-1')}>
                      {row[entry] === 0 ? '—' : formatCount(row[entry], language)}
                    </span>
                  ))}
                  <span role="cell" data-rum-count="sessions" className="text-right font-strong tabular-nums text-tx-0">
                    {formatCount(row.sessions, language)}
                  </span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && <NoMatch>{t('frustration.table.no_match')}</NoMatch>}
        </div>
      </CardState>
    </section>
  );
}

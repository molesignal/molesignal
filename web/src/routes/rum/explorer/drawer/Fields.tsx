import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumRow } from '@/api/rum';

import type { WriteMoment } from '../cells';
import { fieldText, RumFieldValue } from './value';

/**
 * Every field of the row the drawer has not said above, by its name as a query writes it, as a
 * log's drawer lists its fields: a line each, found by a word of its name or of its value.
 */
export function RumDrawerFields({
  names,
  row,
  byName,
  language,
  moment,
}: {
  names: readonly string[];
  row: RumRow;
  byName: ReadonlyMap<string, RumField>;
  language: string;
  moment: WriteMoment;
}) {
  const { t } = useTranslation('rum');
  const [find, setFind] = React.useState('');
  const needle = find.trim().toLowerCase();
  const shown = needle
    ? names.filter(
        (name) => name.toLowerCase().includes(needle) || fieldText(byName.get(name), row[name], language, moment).toLowerCase().includes(needle),
      )
    : names;

  return (
    <>
      <div className="mb-2 flex h-8 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs">
        <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden="true" />
        <input
          type="search"
          value={find}
          onChange={(event) => setFind(event.target.value)}
          placeholder={t('explorer.drawer.find_field')}
          aria-label={t('explorer.drawer.find_field')}
          className="min-w-0 flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:bg-transparent"
        />
        <span className="shrink-0 font-mono tabular-nums text-tx-3">{shown.length}</span>
      </div>
      {shown.length === 0 ? (
        <p className="m-0 py-6 text-center font-sans text-xs text-tx-2">{t('explorer.drawer.no_field_match')}</p>
      ) : (
        <dl data-rum-drawer-fields="" className="m-0 font-sans text-xs">
          {shown.map((name) => (
            <div
              key={name}
              data-rum-drawer-field={name}
              className="grid grid-cols-[minmax(0,38%)_minmax(0,1fr)] gap-3 border-b border-bd-0 py-[7px]"
            >
              <dt className="min-w-0 break-all pl-[6px] font-mono text-tx-2">{name}</dt>
              <dd className="m-0 min-w-0 break-words pr-[6px] text-tx-0">
                <RumFieldValue name={name} row={row} field={byName.get(name)} language={language} moment={moment} />
              </dd>
            </div>
          ))}
        </dl>
      )}
    </>
  );
}

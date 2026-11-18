import { Play } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumField, RumRow } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { describeCell, type WriteMoment } from '../cells';
import type { RumTab } from '../model';
import { isPresent, sessionHref } from './entity';
import { isRecorded, messageOf, pillFields, RUM_SUMMARY, type SummaryTile, titleField } from './facts';
import { fieldText, RumFieldValue } from './value';

/** The ink of a number, as the same word is written in the table. */
const INK = {
  red: 'text-[var(--red-ink)]',
  orange: 'text-[var(--orange-ink)]',
  yellow: 'text-[var(--yellow-ink)]',
  green: 'text-[var(--green-ink)]',
  blue: 'text-[var(--blue-ink)]',
} as const;

/**
 * What the drawer says of a row before its list of fields, as a log's drawer does: what the row
 * says in full, the numbers its type is read by, then where and for whom it happened. A session
 * that was recorded offers its replay.
 */
export function RumRowSummary({
  tab,
  row,
  byName,
  language,
  moment,
}: {
  tab: RumTab;
  row: RumRow;
  byName: ReadonlyMap<string, RumField>;
  language: string;
  /** How a moment is written: in the time zone the page is read in. */
  moment: WriteMoment;
}) {
  const { t } = useTranslation('rum');
  const summary = RUM_SUMMARY[tab];
  const message = messageOf(summary, row);
  const tiles = summary.tiles.filter((tile) => isPresent(row[tile.field]));
  const title = titleField(summary, row);
  const pills = summary.pills
    .map((pill) => ({ pill, fields: pillFields(pill, row, title) }))
    .filter(({ fields }) => fields.length > 0);
  const replay = isRecorded(summary, row) ? sessionHref(row) : null;

  if (!message && tiles.length === 0 && pills.length === 0 && !replay) return null;

  return (
    <div data-rum-drawer-summary="" className="flex min-w-0 flex-col gap-2">
      {message && (
        <div
          data-rum-drawer-message=""
          className="max-h-[34vh] overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-[var(--control-surface)] px-3 py-[10px] font-mono text-xs leading-[1.65] text-tx-0 [overflow-wrap:anywhere]"
        >
          {message}
        </div>
      )}
      {tiles.length > 0 && (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(96px,1fr))] gap-1.5">
          {tiles.map((tile) => (
            <div key={tile.field} data-rum-drawer-tile={tile.field} className="min-w-0 rounded-md bg-[var(--control-surface)] px-3 py-2">
              <div className="truncate font-sans text-xs text-tx-3">{t(`explorer.${tile.label}`)}</div>
              <TileValue tile={tile} row={row} field={byName.get(tile.field)} language={language} moment={moment} />
            </div>
          ))}
        </div>
      )}
      {pills.length > 0 && (
        <dl className="m-0 flex min-w-0 flex-wrap items-center gap-1.5 font-sans text-xs">
          {pills.map(({ pill, fields }) => (
            <div
              key={pill.label}
              data-rum-drawer-pill={fields.join(' ')}
              className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2 py-[3px]"
            >
              <dt className="shrink-0 text-tx-3">{t(`explorer.${pill.label}`)}</dt>
              <dd className={cn('m-0 min-w-0 truncate', inkOf(fields, row, byName, language) ?? 'text-tx-0')}>
                {fields.length === 1 ? (
                  <RumFieldValue name={fields[0]!} row={row} field={byName.get(fields[0]!)} language={language} moment={moment} inline />
                ) : (
                  fields.map((name) => fieldText(byName.get(name), row[name], language, moment)).join(pill.separator ?? ' ')
                )}
              </dd>
            </div>
          ))}
        </dl>
      )}
      {replay && (
        <Link
          to={replay}
          data-testid="rum-drawer-replay"
          className="inline-flex h-8 w-fit items-center gap-1.5 rounded-md bg-[var(--blue-tint)] px-3 font-sans text-xs font-strong text-[var(--blue-ink)] outline-none hover:brightness-95 focus-visible:brightness-95"
        >
          <Play className="h-3 w-3" aria-hidden="true" />
          {t('explorer.drawer.replay')}
        </Link>
      )}
    </div>
  );
}

/** The ink of a pill of one field, as its cell in the table is written: an unhandled error in orange. */
function inkOf(fields: readonly string[], row: RumRow, byName: ReadonlyMap<string, RumField>, language: string): string | null {
  const field = fields.length === 1 ? byName.get(fields[0]!) : undefined;
  const tone = field ? describeCell(field, row[field.name], language)?.tone : null;

  return tone ? INK[tone] : null;
}

/** A tile's number in the ink its cell would have, or the ink of trouble when a count is above nothing. */
function TileValue({
  tile,
  row,
  field,
  language,
  moment,
}: {
  tile: SummaryTile;
  row: RumRow;
  field: RumField | undefined;
  language: string;
  moment: WriteMoment;
}) {
  const value = row[tile.field];
  const cell = field ? describeCell(field, value, language, moment) : null;
  const alarm = tile.alarm && typeof value === 'number' && value > 0 ? tile.alarm : null;
  const ink = alarm ?? cell?.tone ?? null;

  return (
    <div className={cn('truncate font-sans text-lg font-strong tabular-nums', ink ? INK[ink] : 'text-tx-0')}>
      {cell?.text ?? fieldText(field, value, language, moment)}
    </div>
  );
}

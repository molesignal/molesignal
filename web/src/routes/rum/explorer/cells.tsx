import type { RumField } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { DurationBar } from '@/shell/list/DurationBar';
import { httpStatusTone } from '@/viz/trace/model/span';

import { isMoment } from './model';
import { formatDurationMs, formatDurationNs, formatMicros, formatSize, formatShare, regionName } from '../_helpers';

/** The word a value is written in: a colour says what it means, and nothing stands behind it. */
export type CellTone = 'red' | 'orange' | 'yellow' | 'green' | 'blue';

export interface CellText {
  text: string;
  tone: CellTone | null;
}

/** How a moment is written, from its milliseconds since the epoch: as the page writes the time of a row, in its zone. */
export type WriteMoment = (millis: number) => string;

/** How wide the time a row happened at is: the first column of every type, and any other moment. */
export const TIME_TRACK = 'minmax(150px, 0.9fr)';

/** What went wrong is in the strong weight; a good answer, and a mark that something is there, are not. */
const TONE_CLASS: Record<CellTone, string> = {
  red: 'font-strong text-[var(--red-ink)]',
  orange: 'font-strong text-[var(--orange-ink)]',
  yellow: 'font-strong text-[var(--yellow-ink)]',
  green: 'text-[var(--green-ink)]',
  blue: 'text-[var(--blue-ink)]',
};

const STATUS_TONE = { ok: 'green', redirect: 'yellow', client: 'orange', server: 'red' } as const;

const MAX_DOCUMENT_CHARS = 160;

function isEmpty(value: unknown): boolean {
  return value === null || value === undefined || value === '';
}

const isNumeric = (field: RumField) => field.dataType === 'int64' || field.dataType === 'float64';

/** Whether the column holds a number: a column of numbers stands at the right of its cell. A moment is a date and a time. */
export function isNumericColumn(field: RumField): boolean {
  return isNumeric(field) && !field.name.endsWith('status_code') && !isMoment(field);
}

/** The fields that say how long a row itself lasted, one a type. */
const LENGTH_FIELDS = new Set(['session.duration', 'resource.duration', 'long_task.duration']);

/** Whether the column says how long its row lasted: it is drawn as a bar against the longest listed, as a trace's is. */
export function isLengthColumn(field: RumField): boolean {
  return LENGTH_FIELDS.has(field.name) && isNumeric(field);
}

/**
 * What a cell of the table says. `null` for a cell with nothing in it, which the table draws as a
 * dash. A time is in the nanoseconds the SDK sent and is told in the unit it reads best in; a moment
 * is written by `moment`, else in the zone of the reader's preferences.
 */
export function describeCell(field: RumField, value: unknown, language: string, moment?: WriteMoment): CellText | null {
  if (isEmpty(value)) return null;

  const plain = (text: string, tone: CellTone | null = null): CellText => ({ text, tone });

  if (field.name.endsWith('status_code') && typeof value === 'number') {
    return plain(String(value), value === 0 ? 'red' : STATUS_TONE[httpStatusTone(value)]);
  }

  if (field.name === 'geo.country_iso_code' && typeof value === 'string') {
    return plain(regionName(value, language));
  }

  if (field.name === 'error.handling') return plain(String(value), value === 'unhandled' ? 'orange' : null);
  if (field.name === 'action.frustration.type') return plain(String(value), 'yellow');

  if (field.dataType === 'bool') {
    const on = value === true || value === 'true';

    if (field.name === 'error.is_crash') return plain(String(on), on ? 'red' : null);
    if (field.name === 'session.has_replay') return plain(String(on), on ? 'blue' : null);

    return plain(String(on));
  }

  if (typeof value === 'number') {
    switch (field.unit) {
      case 'epoch_ms':
        return plain(moment ? moment(value) : formatMicros(value * 1_000));
      case 'ns':
        return plain(formatDurationNs(value));
      case 'ms':
        return plain(formatDurationMs(value));
      case 'bytes':
        return plain(formatSize(value));
      case 'percent':
        return plain(formatShare(value / 100));
      case 'score':
        return plain(String(Number(value.toFixed(3))));
      default:
        return plain(Number.isInteger(value) ? value.toLocaleString(language) : String(value));
    }
  }

  if (typeof value === 'string') return plain(value);

  const document = JSON.stringify(value);

  return plain(document.length > MAX_DOCUMENT_CHARS ? `${document.slice(0, MAX_DOCUMENT_CHARS)}…` : document);
}

/** How a plain cell is written: in the body grey, as every column of the table; an id and a moment with tabular figures, as the times. */
function plainCellClass(field: RumField): string {
  return field.name.endsWith('.id') || isMoment(field) ? 'tabular-nums text-tx-0' : 'text-tx-0';
}

/** A cell of the table: its words in their colour, or a dash; how long the row lasted, also as a bar. */
export function RumCell({
  field,
  value,
  language,
  moment,
  longest,
  failed = false,
}: {
  field: RumField;
  value: unknown;
  language: string;
  /** How a moment is written: as the time of the row is. */
  moment?: WriteMoment;
  /** The longest time of the rows listed, for a column that says how long its row lasted. */
  longest?: number;
  /** The row went wrong: its bar is red. */
  failed?: boolean;
}) {
  const cell = describeCell(field, value, language, moment);

  if (!cell) return <span className="text-tx-4">—</span>;

  if (longest !== undefined && typeof value === 'number' && isLengthColumn(field)) {
    return <DurationBar text={cell.text} value={value} longest={longest} failed={failed} />;
  }

  return (
    <span
      className={cn('truncate', cell.tone ? TONE_CLASS[cell.tone] : plainCellClass(field), isNumericColumn(field) && 'tabular-nums')}
      title={cell.text}
    >
      {cell.text}
    </span>
  );
}

/** How wide a column is, and which side its words stand on. */
export interface ColumnLayout {
  track: string;
  align: 'start' | 'end';
}

const WIDE = /(^|\.)(url|message|selector|stack)$|url_path$|target\.name$|^view\.name$/;

export function columnLayout(field: RumField): ColumnLayout {
  // As wide as a trace list's: the time, then its bar.
  if (isLengthColumn(field)) return { track: '148px', align: 'start' };
  // As wide as the time every row starts with, and on its side.
  if (isMoment(field)) return { track: TIME_TRACK, align: 'start' };
  if (isNumericColumn(field)) return { track: 'minmax(84px, 0.6fr)', align: 'end' };
  if (field.dataType === 'bool') return { track: 'minmax(72px, 0.5fr)', align: 'start' };
  if (field.name.endsWith('.id')) return { track: 'minmax(120px, 1fr)', align: 'start' };
  if (WIDE.test(field.name)) return { track: 'minmax(180px, 2.4fr)', align: 'start' };

  return { track: 'minmax(96px, 1fr)', align: 'start' };
}

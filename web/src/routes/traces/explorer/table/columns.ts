import type { TraceListSort } from '@/api/web';

/** A column the trace table draws from the trace itself. */
export type TraceColumnKey = 'status' | 'http' | 'start' | 'service' | 'operation' | 'duration' | 'spans' | 'errors' | 'trace';

/** The columns a trace carries, in the order the table opens with. */
export const TRACE_COLUMNS: readonly TraceColumnKey[] = ['status', 'http', 'start', 'service', 'operation', 'duration', 'spans', 'errors', 'trace'];

/**
 * How a column is named where the reader's arrangement is kept: a column the
 * trace carries by its key behind an underscore, a field of the root span by
 * its own name. No field of the root can begin with an underscore, so the two
 * never meet.
 */
export const traceColumnId = (key: TraceColumnKey) => `_${key}`;

/** What the table opens with, until the reader arranges it. */
export const DEFAULT_TRACE_COLUMN_IDS: readonly string[] = TRACE_COLUMNS.map(traceColumnId);

const KEY_BY_ID = new Map(TRACE_COLUMNS.map((key) => [traceColumnId(key), key]));

/** A column of the table: one the trace carries, or (without a key) a field of its root span. */
export interface TraceTableColumn {
  id: string;
  key: TraceColumnKey | null;
}

export function traceColumnKey(id: string): TraceColumnKey | null {
  return KEY_BY_ID.get(id) ?? null;
}

/** The columns an arrangement names, in its order, leaving out the fields of the root that cannot be shown. */
export function traceTableColumns(ids: readonly string[], rootFields: ReadonlySet<string>): TraceTableColumn[] {
  return ids.flatMap((id) => {
    const key = traceColumnKey(id);
    return key || rootFields.has(id) ? [{ id, key }] : [];
  });
}

/**
 * The track each column takes. A name column shares what is left; a figure
 * takes what it needs. At their least, the columns the table opens with fit
 * the results pane of a 1280px window with the pane's scrollbar beside them.
 */
const TRACKS: Record<TraceColumnKey, string> = {
  status: '52px',
  http: '48px',
  start: '100px',
  service: 'minmax(96px,0.9fr)',
  operation: 'minmax(128px,1.5fr)',
  duration: '148px',
  spans: '60px',
  errors: '52px',
  trace: 'minmax(96px,0.9fr)',
};

const ROOT_TRACK = 'minmax(110px,0.8fr)';

/** The tracks of the table, one a column; with no column to share what is left, an empty one takes it. */
export function traceTableTracks(columns: readonly TraceTableColumn[]): string[] {
  const tracks = columns.map((column) => (column.key ? TRACKS[column.key] : ROOT_TRACK));
  return tracks.some((track) => track.includes('fr')) ? tracks : [...tracks, 'minmax(0,1fr)'];
}

/**
 * The orders a column sorts by, the first on a first click and the second
 * on a click on a column already sorted by it. A column with one order
 * keeps it.
 */
export const COLUMN_SORTS: Partial<Record<TraceColumnKey, readonly TraceListSort[]>> = {
  start: ['latest', 'earliest'],
  duration: ['duration_desc', 'duration_asc'],
  spans: ['span_count_desc'],
  errors: ['errors_desc'],
};

/** The order a click on a column asks for. */
export function nextColumnSort(column: TraceColumnKey, current: TraceListSort): TraceListSort | null {
  const sorts = COLUMN_SORTS[column];
  if (!sorts) return null;

  const at = sorts.indexOf(current);
  return at === -1 ? sorts[0]! : sorts[(at + 1) % sorts.length]!;
}

/** Which way a column is sorted now, if it is. */
export function columnSortDirection(column: TraceColumnKey, current: TraceListSort): 'descending' | 'ascending' | null {
  const sorts = COLUMN_SORTS[column];
  if (!sorts?.includes(current)) return null;
  return current === 'earliest' || current === 'duration_asc' ? 'ascending' : 'descending';
}

/** Fields that are columns of their own, or the trace's internal counts: never a field of the root to add. */
const FIXED_FIELDS = new Set(['trace_id', 'span_id', 'parent_span_id', 'service.name', 'name', 'duration_ns', 'span_count', 'error_count', 'status_code']);

/** Whether a field of the stream can be put beside the traces as a column: a field of the root span, not a document. */
export function isRootColumnCandidate(field: { name: string; dataType: string; physical?: boolean }): boolean {
  return (
    field.physical !== false
    && field.dataType !== 'json'
    && !FIXED_FIELDS.has(field.name)
    && !field.name.startsWith('molesignal.')
    && !field.name.startsWith('_')
  );
}

import type { TraceListSort } from '@/api/web';

/** The orders the trace list is asked in; the column heads of the table pick one. */
const TRACE_LIST_SORT_VALUES = new Set<TraceListSort>([
  'latest',
  'earliest',
  'duration_desc',
  'duration_asc',
  'span_count_desc',
  'errors_desc',
]);

export function parseTraceListSort(value: string | null): TraceListSort {
  return value !== null && TRACE_LIST_SORT_VALUES.has(value as TraceListSort)
    ? (value as TraceListSort)
    : 'latest';
}

export function writeTraceListSort(
  current: URLSearchParams,
  sort: TraceListSort,
): URLSearchParams {
  const next = new URLSearchParams(current);
  if (sort === 'latest') next.delete('sort');
  else next.set('sort', sort);
  return next;
}

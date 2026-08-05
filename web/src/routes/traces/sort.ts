import type { TraceListSort } from '@/api/web';

export const TRACE_LIST_SORT_OPTIONS: ReadonlyArray<{
  value: TraceListSort;
  labelKey: string;
}> = [
  { value: 'latest', labelKey: 'explore.sort.latest' },
  { value: 'earliest', labelKey: 'explore.sort.earliest' },
  { value: 'duration_desc', labelKey: 'explore.sort.duration_desc' },
  { value: 'duration_asc', labelKey: 'explore.sort.duration_asc' },
  { value: 'span_count_desc', labelKey: 'explore.sort.span_count_desc' },
  { value: 'errors_desc', labelKey: 'explore.sort.errors_desc' },
];

const TRACE_LIST_SORT_VALUES = new Set<TraceListSort>(
  TRACE_LIST_SORT_OPTIONS.map((option) => option.value),
);

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

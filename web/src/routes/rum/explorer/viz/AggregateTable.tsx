import { ArrowDown, ArrowUp } from 'lucide-react';
import * as React from 'react';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatMeasure, groupLabel, rankedRows } from './answer';
import type { VizFilters } from './labels';
import { measureName, type RumVizSpec } from './spec';
import { GRADE_TEXT, vitalGrade } from './tone';

const GROUP_TRACK = 'minmax(160px, 2fr)';
const NUMBER_TRACK = 'minmax(96px, 1fr)';

/**
 * A row for each group and a column for each number, the groups first. A head sorts the table:
 * the server ranks by it, so the rows are the first of that order, not the sorted first of
 * another. A group that can be filtered by narrows the query when it is picked.
 */
export function RumAggregateTable({
  answer,
  spec,
  groupHeads,
  measureHeads,
  language,
  none,
  filters,
  onSort,
}: {
  answer: RumAggregate;
  spec: RumVizSpec;
  /** What the group columns are headed with, in order. */
  groupHeads: readonly string[];
  /** What the number columns are headed with, in order. */
  measureHeads: readonly string[];
  language: string;
  none: string;
  filters: VizFilters;
  onSort: (by: string, descending: boolean) => void;
}) {
  const rows = React.useMemo(() => rankedRows(answer), [answer]);
  const tracks = [...spec.groups.map(() => GROUP_TRACK), ...spec.measures.map(() => NUMBER_TRACK)].join(' ');
  // Chosen, or the one the server answers in unasked: the first number, the largest first.
  const sorted = spec.sort ?? { by: measureName(spec.measures[0]!), descending: true };

  const heads = [
    ...spec.groups.map((field, index) => ({ by: field, label: groupHeads[index] ?? field, numeric: false })),
    ...spec.measures.map((measure, index) => ({ by: measureName(measure), label: measureHeads[index] ?? measure.fn, numeric: true })),
  ];

  return (
    <div data-rum-viz-body="table" role="table" className="min-h-0 flex-1 overflow-auto">
      <div
        role="row"
        data-rum-table-head=""
        className="sticky top-0 z-10 grid h-8 min-w-max items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
        style={{ gridTemplateColumns: tracks }}
      >
        {heads.map((head) => {
          const active = head.by === sorted.by;

          return (
            <span
              key={head.by}
              role="columnheader"
              aria-sort={active ? (sorted.descending ? 'descending' : 'ascending') : 'none'}
              className={cn('flex min-w-0', head.numeric && 'justify-end')}
            >
              <button
                type="button"
                data-rum-sort={head.by}
                title={head.by}
                // A number is looked for from the largest down, a name from the first up.
                onClick={() => onSort(head.by, active ? !sorted.descending : head.numeric)}
                className="inline-flex min-w-0 items-center gap-1 rounded-sm font-display hover:text-indigo-soft focus-visible:text-indigo-soft focus-visible:outline-none"
              >
                <span className="truncate">{head.label}</span>
                {active && (sorted.descending ? <ArrowDown aria-hidden="true" className="h-3 w-3 shrink-0" /> : <ArrowUp aria-hidden="true" className="h-3 w-3 shrink-0" />)}
              </button>
            </span>
          );
        })}
      </div>
      {rows.map((row) => (
        <div
          key={row.key}
          role="row"
          data-rum-viz-row=""
          className="grid h-8 min-w-max items-center gap-3 border-b border-[var(--row-rule)] px-3 font-sans text-xs"
          style={{ gridTemplateColumns: tracks }}
        >
          {spec.groups.map((field, index) => {
            const value = row.group[index] ?? null;
            const label = groupLabel(field, value, language, none);

            return (
              <span key={field} role="cell" className="flex min-w-0">
                {value !== null && filters.can(field, value) ? (
                  <button
                    type="button"
                    aria-pressed={filters.isFiltered(field, value)}
                    onClick={() => filters.toggle(field, value)}
                    className={cn(
                      'min-w-0 truncate rounded-sm text-left hover:text-indigo-soft focus-visible:text-indigo-soft focus-visible:outline-none',
                      filters.isFiltered(field, value) ? 'font-strong text-indigo-soft' : 'text-tx-0',
                    )}
                  >
                    {label}
                  </button>
                ) : (
                  <span className={cn('truncate', value === null ? 'text-tx-3' : 'text-tx-0')}>{label}</span>
                )}
              </span>
            );
          })}
          {spec.measures.map((measure, index) => {
            const raw = row.values[index] ?? null;
            const grade = vitalGrade(measure, raw);

            return (
              <span key={measureName(measure)} role="cell" className={cn('truncate text-right tabular-nums', grade ? GRADE_TEXT[grade] : 'text-tx-0')}>
                {formatMeasure(raw, answer.measures[index]?.unit ?? 'none', language)}
              </span>
            );
          })}
        </div>
      ))}
    </div>
  );
}

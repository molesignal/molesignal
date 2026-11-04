import * as React from 'react';

import type { RumAggregate } from '@/api/rum';

import { formatMeasure, groupLabel, rankedRows } from './answer';
import type { VizFilters } from './labels';
import type { RumVizSpec } from './spec';
import { GRADE_TEXT, vitalGrade } from './tone';
import { type BarRow, BarRows } from '../../overview/BarRows';

/**
 * The groups ranked by one number: each a bar as long as its number is next to the largest, the
 * number after it. A group that can be filtered by is a button: picked, it narrows the query to
 * it; picked again, it lets go.
 */
export function RumTopList({
  answer,
  spec,
  title,
  language,
  none,
  filters,
}: {
  answer: RumAggregate;
  spec: RumVizSpec;
  title: string;
  language: string;
  none: string;
  filters: VizFilters;
}) {
  const field = spec.groups[0]!;
  const measure = spec.measures[0]!;
  const unit = answer.measures[0]?.unit ?? 'none';

  const rows = React.useMemo<BarRow[]>(
    () =>
      rankedRows(answer).map((row) => {
        const value = row.group[0] ?? null;
        const raw = row.values[0] ?? null;
        const grade = vitalGrade(measure, raw);
        const selectable = value !== null && filters.can(field, value);

        return {
          key: row.key,
          label: groupLabel(field, value, language, none),
          count: typeof raw === 'number' && Number.isFinite(raw) ? Math.max(0, raw) : 0,
          text: formatMeasure(raw, unit, language),
          ...(grade ? { textClassName: GRADE_TEXT[grade] } : {}),
          ...(selectable ? { onSelect: () => filters.toggle(field, value), active: filters.isFiltered(field, value) } : {}),
        };
      }),
    [answer, field, filters, language, measure, none, unit],
  );

  return (
    <div data-rum-viz-body="toplist" className="min-h-0 flex-1 overflow-auto py-3">
      <h2 className="px-3 pb-1 font-sans text-sm font-strong text-tx-0">{title}</h2>
      <BarRows rows={rows} total={0} language={language} labelWidth={220} testId="rum-toplist" />
    </div>
  );
}

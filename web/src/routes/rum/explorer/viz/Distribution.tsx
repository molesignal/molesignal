import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumDistribution } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatMeasure } from './answer';
import { markerPlaces } from './distributionModel';

/** The percentiles marked on the bars. */
const MARKS = ['p50', 'p75', 'p95', 'p99'] as const;

/** The height of a line of the names of the marks, in pixels. */
const NAME_LINE = 16;

/** Past this share of the chart a name stands before its mark, or it would be cut off. */
const NAME_FLIPS_AT = 0.9;

/**
 * How the values of a number fall across the rows the query matches: a bar for each stretch of
 * values, as tall as it has rows, with the percentiles marked over them. The last bar counts what
 * lies past the others, so that one value far off does not squeeze them into a bar.
 */
export function RumDistributionChart({ answer, title, language }: { answer: RumDistribution; title: string; language: string }) {
  const { t } = useTranslation('rum');
  const { bars, unit } = answer;
  const highest = React.useMemo(() => bars.reduce((most, bar) => Math.max(most, bar.rows), 0), [bars]);
  const value = (number: number | null | undefined) => formatMeasure(number ?? null, unit, language);
  const marks = markerPlaces(answer, MARKS);
  const nameLines = marks.reduce((lines, mark) => Math.max(lines, mark.line + 1), 0);
  const last = bars[bars.length - 1];
  const first = bars[0];

  return (
    <div data-rum-viz-body="distribution" className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-3">
      <h2 className="m-0 font-sans text-sm font-strong text-tx-0">{title}</h2>
      <dl className="m-0 flex flex-wrap gap-x-6 gap-y-1 font-sans text-xs">
        {[
          ['rows', answer.rows.toLocaleString(language)],
          ['p50', value(answer.p50)],
          ['p75', value(answer.p75)],
          ['p95', value(answer.p95)],
          ['p99', value(answer.p99)],
          ['max', value(answer.max)],
        ].map(([name, text]) => (
          <div key={name} className="flex items-baseline gap-1.5">
            <dt className="text-tx-3">{t(`explorer.viz.distribution.${name}`)}</dt>
            <dd data-rum-distribution-figure={name} className="m-0 tabular-nums text-tx-0">
              {text}
            </dd>
          </div>
        ))}
      </dl>
      <div className="relative min-h-[220px] flex-1">
        <div
          role="img"
          aria-label={t('explorer.viz.distribution.aria', { title, rows: answer.rows.toLocaleString(language) })}
          className="absolute inset-x-0 bottom-0 flex items-end gap-px"
          style={{ top: nameLines * NAME_LINE }}
        >
          {bars.map((bar, index) => (
            <div
              key={bar.from}
              data-rum-distribution-bar={index}
              title={t('explorer.viz.distribution.bar', {
                from: value(bar.from),
                to: value(bar.to),
                rows: bar.rows.toLocaleString(language),
              })}
              className="min-w-0 flex-1 rounded-t-[2px] bg-[var(--chart-1)]"
              style={{ height: highest > 0 && bar.rows > 0 ? `${Math.max(1, (bar.rows / highest) * 100)}%` : 0 }}
            />
          ))}
        </div>
        {marks.map((mark) => (
          <div
            key={mark.name}
            aria-hidden="true"
            data-rum-distribution-mark={mark.name}
            className="pointer-events-none absolute inset-y-0 w-px bg-[var(--chart-2)]"
            style={{ left: `${mark.place * 100}%` }}
          >
            <span
              className={cn('absolute type-micro font-sans text-tx-3', mark.place > NAME_FLIPS_AT ? 'right-1' : 'left-1')}
              style={{ top: mark.line * NAME_LINE }}
            >
              {t(`explorer.viz.distribution.${mark.name}`)}
            </span>
          </div>
        ))}
      </div>
      {first && last && (
        <div className="flex justify-between gap-3 font-sans text-xs tabular-nums text-tx-3">
          <span>{value(first.from)}</span>
          <span>{value(last.to)}</span>
        </div>
      )}
    </div>
  );
}

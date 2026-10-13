import type { MetricType } from '@/api/metricsCatalog';
import { metricTypeAbbreviation } from '@/lib/metricTypes';

/** The ink tones, as on a trace's HTTP method: AA on a row at rest, under the pointer and selected. */
const METRIC_TYPE_COLOR: Record<MetricType, string> = {
  counter: 'text-[var(--orange-ink)]',
  histogram: 'text-[var(--green-ink)]',
  gauge: 'text-[var(--blue-ink)]',
};

export function MetricTypePrefix({
  type,
  label,
}: {
  type: MetricType;
  label: string;
}) {
  return (
    <>
      <span
        aria-hidden="true"
        title={label}
        className={`type-micro w-8 shrink-0 font-mono font-semibold ${METRIC_TYPE_COLOR[type]}`}
      >
        {metricTypeAbbreviation(type)}
      </span>
      <span className="sr-only">{label}: </span>
    </>
  );
}

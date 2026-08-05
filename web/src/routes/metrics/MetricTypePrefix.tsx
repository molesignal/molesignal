import type { MetricType } from '@/api/metricsCatalog';
import { metricTypeAbbreviation } from '@/lib/metricTypes';

const METRIC_TYPE_COLOR: Record<MetricType, string> = {
  counter: 'text-orange-soft',
  histogram: 'text-green-soft',
  gauge: 'text-blue-soft',
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

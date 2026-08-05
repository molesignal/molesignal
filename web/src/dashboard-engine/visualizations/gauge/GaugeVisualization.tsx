import {
  buildThresholdIntervals,
  normalizeGaugeRange,
  resolveThresholdColor,
} from './geometry';
import { RadialGauge } from './RadialGauge';
import { formatFieldValue } from '../../fieldConfig';
import type {
  DataField,
  DataFrame,
  FieldConfig,
} from '../../schema';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import {
  calculationOption,
  reduceNumericValues,
  type Calculation,
} from '../shared/reduction';
import type { VisualizationProps } from '../shared/types';

export type GaugeVisualizationProps = VisualizationProps;

interface GaugeValue {
  field: DataField;
  value: number;
}

export function GaugeVisualization({
  data,
  options,
  height,
}: GaugeVisualizationProps) {
  const calculation = calculationOption(options.calculation);
  const item = firstGaugeValue(data.frames, calculation);
  if (!item) {
    return <EmptyVisualization />;
  }

  const range = normalizeGaugeRange(
    item.field.config?.min,
    item.field.config?.max,
    item.value,
  );
  const normalizedConfig: FieldConfig = {
    ...(item.field.config ?? {}),
    min: range.min,
    max: range.max,
  };
  const display = formatFieldValue(item.value, normalizedConfig);
  const rangeConfig: FieldConfig = {
    ...normalizedConfig,
    color: undefined,
    mappings: [],
    thresholds: undefined,
  };
  const formatRangeValue = (value: number) =>
    formatFieldValue(value, rangeConfig).text;
  const thresholdIntervals = buildThresholdIntervals(
    normalizedConfig.thresholds,
    range,
  ).map((interval) => ({
    ...interval,
    label: interval.label ?? formatRangeValue(interval.start),
  }));
  const name = item.field.config?.displayName ?? item.field.name;

  return (
    <RadialGauge
      value={item.value}
      valueText={display.text}
      name={name}
      range={range}
      minimumText={formatRangeValue(range.min)}
      maximumText={formatRangeValue(range.max)}
      color={
        display.color ??
        resolveThresholdColor(
          item.value,
          normalizedConfig.thresholds,
          range,
        )
      }
      thresholdIntervals={thresholdIntervals}
      showThresholdMarkers={options.showThresholdMarkers !== false}
      showThresholdLabels={options.showThresholdLabels === true}
      height={height}
    />
  );
}

export function firstGaugeValue(
  frames: readonly DataFrame[],
  calculation: Calculation,
): GaugeValue | null {
  for (const frame of frames) {
    for (const field of frame.fields) {
      if (field.type !== 'number') continue;
      const value = reduceGaugeValues(field.values, calculation);
      if (value !== null) return { field, value };
    }
  }
  return null;
}

export function reduceGaugeValues(
  values: readonly unknown[],
  calculation: Calculation,
): number | null {
  return reduceNumericValues(values, calculation);
}

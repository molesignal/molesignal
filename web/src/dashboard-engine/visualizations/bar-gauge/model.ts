import { formatFieldValue, type DisplayValue } from '../../fieldConfig';
import type { DataField, DataFrame, FieldConfig } from '../../schema';
import { visualizationColor } from '../shared/colors';
import {
  normalizeValueRange,
  valueRatio,
  type ValueRange,
} from '../shared/range';
import {
  numericDisplayValues,
  type Calculation,
} from '../shared/reduction';
import {
  buildThresholdIntervals,
  resolveThresholdColor,
  thresholdMarkerValues,
  type ThresholdInterval,
} from '../shared/thresholds';

export interface BarGaugeValue {
  key: string;
  field: DataField;
  name: string;
  value: number;
  display: DisplayValue;
  range: ValueRange;
  ratio: number;
  color: string;
  intervals: ThresholdInterval[];
  markers: number[];
  minimumText: string;
  maximumText: string;
}

export function prepareBarGaugeValues(
  frames: readonly DataFrame[],
  calculation: Calculation,
): BarGaugeValue[] {
  return numericDisplayValues(frames, calculation).map((item) => {
    const range = normalizeValueRange(
      item.field.config?.min,
      item.field.config?.max,
      item.value,
    );
    const config: FieldConfig = {
      ...(item.field.config ?? {}),
      min: range.min,
      max: range.max,
    };
    const display = formatFieldValue(item.value, config);
    const rangeConfig: FieldConfig = {
      ...config,
      color: undefined,
      mappings: [],
      thresholds: undefined,
    };
    return {
      key: item.key,
      field: item.field,
      name: item.field.config?.displayName ?? item.field.name,
      value: item.value,
      display,
      range,
      ratio: valueRatio(item.value, range),
      color:
        display.color ??
        resolveThresholdColor(item.value, config.thresholds, range) ??
        config.color?.value ??
        visualizationColor(item.key),
      intervals: buildThresholdIntervals(config.thresholds, range),
      markers: thresholdMarkerValues(config.thresholds, range),
      minimumText: formatFieldValue(range.min, rangeConfig).text,
      maximumText: formatFieldValue(range.max, rangeConfig).text,
    };
  });
}

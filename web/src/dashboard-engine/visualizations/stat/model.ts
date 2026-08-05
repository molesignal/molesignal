import { formatFieldValue, type DisplayValue } from '../../fieldConfig';
import type { DataField, DataFrame } from '../../schema';
import { visualizationColor } from '../shared/colors';
import {
  numericDisplayValues,
  type Calculation,
} from '../shared/reduction';

export interface StatValue {
  key: string;
  field: DataField;
  name: string;
  value: number;
  display: DisplayValue;
  color: string;
  sparkline: number[];
  percentChange: number | null;
}

export function prepareStatValues(
  frames: readonly DataFrame[],
  calculation: Calculation,
): StatValue[] {
  return numericDisplayValues(frames, calculation).map((item) => {
    const name = item.field.config?.displayName ?? item.field.name;
    const display = formatFieldValue(item.value, item.field.config);
    return {
      key: item.key,
      field: item.field,
      name,
      value: item.value,
      display,
      color:
        display.color ??
        item.field.config?.color?.value ??
        visualizationColor(item.key),
      sparkline: item.values,
      percentChange: finitePercentChange(item.values),
    };
  });
}

export function finitePercentChange(values: readonly number[]): number | null {
  if (values.length < 2) return null;
  const first = values[0];
  const last = values.at(-1);
  if (
    first === undefined ||
    last === undefined ||
    !Number.isFinite(first) ||
    !Number.isFinite(last) ||
    first === 0
  ) {
    return null;
  }
  return ((last - first) / Math.abs(first)) * 100;
}

export function formatPercentChange(value: number): string {
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(Math.abs(value) >= 100 ? 0 : 1)}%`;
}

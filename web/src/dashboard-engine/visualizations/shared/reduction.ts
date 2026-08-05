import type { DataField, DataFrame } from '../../schema';

export const CALCULATIONS = [
  'last',
  'min',
  'max',
  'mean',
  'avg',
  'sum',
] as const;

export type Calculation = (typeof CALCULATIONS)[number];

export interface NumericDisplayValue {
  key: string;
  frame: DataFrame;
  field: DataField;
  value: number;
  values: number[];
}

export function calculationOption(
  value: unknown,
  fallback: Calculation = 'last',
): Calculation {
  return typeof value === 'string' &&
    CALCULATIONS.includes(value as Calculation)
    ? (value as Calculation)
    : fallback;
}

export function finiteNumbers(values: readonly unknown[]): number[] {
  return values.filter(
    (value): value is number =>
      typeof value === 'number' && Number.isFinite(value),
  );
}

export function reduceNumericValues(
  values: readonly unknown[],
  calculation: Calculation,
): number | null {
  const numbers = finiteNumbers(values);
  if (numbers.length === 0) return null;
  if (calculation === 'min') return Math.min(...numbers);
  if (calculation === 'max') return Math.max(...numbers);
  if (calculation === 'mean' || calculation === 'avg') {
    return numbers.reduce((sum, value) => sum + value, 0) / numbers.length;
  }
  if (calculation === 'sum') {
    return numbers.reduce((sum, value) => sum + value, 0);
  }
  return numbers.at(-1) ?? null;
}

export function numericDisplayValues(
  frames: readonly DataFrame[],
  calculation: Calculation,
): NumericDisplayValue[] {
  return frames.flatMap((frame) =>
    frame.fields.flatMap((field) => {
      if (field.type !== 'number') return [];
      const values = finiteNumbers(field.values);
      const value = reduceNumericValues(values, calculation);
      return value === null
        ? []
        : [{ key: `${frame.refId}:${field.id}`, frame, field, value, values }];
    }),
  );
}

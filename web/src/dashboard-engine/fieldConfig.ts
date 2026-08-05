import type {
  DataField,
  DataFrame,
  FieldConfig,
  FieldOverride,
  ValueMapping,
} from './schema';

export interface DisplayValue {
  text: string;
  color?: string | undefined;
  icon?: string | undefined;
  numeric?: number | undefined;
}

export function applyFieldConfig(
  frames: readonly DataFrame[],
  defaults: FieldConfig,
  overrides: readonly FieldOverride[],
): DataFrame[] {
  return frames.map((frame) => ({
    ...frame,
    fields: frame.fields.map((field) => ({
      ...field,
      config: resolveFieldConfig(field, frame.refId, defaults, overrides),
    })),
  }));
}

export function resolveFieldConfig(
  field: DataField,
  refId: string,
  defaults: FieldConfig,
  overrides: readonly FieldOverride[],
): FieldConfig {
  let config = { ...defaults, ...(field.config ?? {}) };
  for (const override of overrides) {
    if (!fieldMatches(field, refId, override)) continue;
    for (const property of override.properties) {
      config = { ...config, [property.id]: property.value };
    }
  }
  return config;
}

export function formatFieldValue(
  value: unknown,
  config: FieldConfig = {},
): DisplayValue {
  const mapped = (config.mappings ?? [])
    .map((mapping) => mapValue(value, mapping))
    .find((result) => result !== null);
  if (mapped) {
    return {
      text: mapped.text ?? stringifyValue(value, config),
      color: mapped.color,
      icon: mapped.icon,
      numeric: numericValue(value),
    };
  }
  const numeric = numericValue(value);
  return {
    text:
      value === null || value === undefined || value === ''
        ? config.noValue ?? '—'
        : stringifyValue(value, config),
    color: numeric === undefined ? undefined : thresholdColor(numeric, config),
    numeric,
  };
}

export function thresholdColor(
  value: number,
  config: FieldConfig,
): string | undefined {
  if (config.color?.mode === 'fixed') return config.color.value;
  const thresholds = config.thresholds;
  if (!thresholds || thresholds.steps.length === 0) return undefined;
  const min = config.min ?? 0;
  const max = config.max ?? 100;
  const comparable =
    thresholds.mode === 'percentage' && max !== min
      ? ((value - min) / (max - min)) * 100
      : value;
  return [...thresholds.steps]
    .sort((left, right) => (left.value ?? -Infinity) - (right.value ?? -Infinity))
    .reduce<string | undefined>(
      (color, step) =>
        step.value === null || comparable >= step.value ? step.color : color,
      undefined,
    );
}

function fieldMatches(
  field: DataField,
  refId: string,
  override: FieldOverride,
): boolean {
  const matcher = override.matcher;
  if (matcher.type === 'field_name') return field.name === matcher.value;
  if (matcher.type === 'field_type') return field.type === matcher.value;
  if (matcher.type === 'query_ref') return refId === matcher.value;
  try {
    return new RegExp(matcher.value).test(field.name);
  } catch {
    return false;
  }
}

function mapValue(
  value: unknown,
  mapping: ValueMapping,
): ValueMapping['result'] | null {
  if (mapping.type === 'value') {
    return Object.is(value, mapping.value) ||
      String(value) === String(mapping.value)
      ? mapping.result
      : null;
  }
  if (mapping.type === 'range') {
    const numeric = numericValue(value);
    if (numeric === undefined) return null;
    if (mapping.from !== undefined && numeric < mapping.from) return null;
    if (mapping.to !== undefined && numeric > mapping.to) return null;
    return mapping.result;
  }
  if (mapping.type === 'regex') {
    try {
      return new RegExp(mapping.pattern).test(String(value))
        ? mapping.result
        : null;
    } catch {
      return null;
    }
  }
  const matches =
    (mapping.match === 'null' && (value === null || value === undefined)) ||
    (mapping.match === 'nan' &&
      typeof value === 'number' &&
      Number.isNaN(value)) ||
    (mapping.match === 'true' && value === true) ||
    (mapping.match === 'false' && value === false) ||
    (mapping.match === 'empty' && value === '');
  return matches ? mapping.result : null;
}

function stringifyValue(value: unknown, config: FieldConfig): string {
  const numeric = numericValue(value);
  if (numeric !== undefined) {
    const formatted = numeric.toLocaleString(undefined, {
      maximumFractionDigits: config.decimals ?? 3,
      minimumFractionDigits: config.decimals,
    });
    return appendUnit(formatted, config.unit);
  }
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

function appendUnit(value: string, unit?: string): string {
  if (!unit || unit === 'none' || unit === 'short') return value;
  const suffixes: Record<string, string> = {
    percent: '%',
    percentunit: '%',
    milliseconds: ' ms',
    ms: ' ms',
    seconds: ' s',
    s: ' s',
    bytes: ' B',
    requests_per_second: ' req/s',
  };
  return `${value}${suffixes[unit] ?? ` ${unit}`}`;
}

function numericValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

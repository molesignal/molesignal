import { renderLegendFormat } from '../dataframe';
import type { DataFrame, PanelQuery } from '../schema';
import { resolveQueryLegendMode } from './legend';

/**
 * Legend aliases affect presentation only. Keeping them out of the executable
 * query lets the dashboard reuse the current data while an alias is edited.
 */
export function toExecutablePanelQuery(query: PanelQuery): PanelQuery {
  const { legend: _legend, ...executableQuery } = query;
  return executableQuery;
}

/** Applies the current query alias to raw cached frames without mutating them. */
export function applyQueryPresentation(
  frames: DataFrame[],
  query: PanelQuery,
): DataFrame[] {
  const pattern = query.legend;
  if (query.dataSourceType === 'metrics') {
    return applyMetricLegend(frames, pattern);
  }
  if (
    query.dataSourceType !== 'profiles' ||
    resolveQueryLegendMode(pattern) !== 'custom'
  ) return frames;

  return frames.map((frame) => {
    const name = pattern!;
    return frame.name === name ? frame : { ...frame, name };
  });
}

function applyMetricLegend(
  frames: DataFrame[],
  pattern: string | undefined,
): DataFrame[] {
  const mode = resolveQueryLegendMode(pattern);
  const labelSets = frames.map(numericFieldLabels);
  const common = mode === 'auto' ? commonLabels(labelSets) : {};

  return frames.map((frame, index) => {
    const labels = labelSets[index] ?? {};
    const fallback = numericFieldName(frame, index);
    const name =
      mode === 'custom'
        ? renderLegendFormat(pattern!, labels)
        : formatLabels(
            mode === 'auto' ? withoutLabels(labels, common) : labels,
            fallback,
          );
    return frame.name === name ? frame : { ...frame, name };
  });
}

function numericFieldLabels(
  frame: DataFrame,
): Readonly<Record<string, string>> {
  return (
    frame.fields.find(
      (field) => field.type === 'number' && field.labels,
    )?.labels ?? {}
  );
}

function numericFieldName(frame: DataFrame, index: number): string {
  return (
    frame.fields.find((field) => field.type === 'number')?.name ??
    frame.name ??
    `Series ${index + 1}`
  );
}

function commonLabels(
  labelSets: ReadonlyArray<Readonly<Record<string, string>>>,
): Record<string, string> {
  if (labelSets.length === 0) return {};
  const common = { ...labelSets[0] };
  for (const labels of labelSets.slice(1)) {
    for (const key of Object.keys(common)) {
      if (labels[key] !== common[key]) delete common[key];
    }
  }
  return common;
}

function withoutLabels(
  labels: Readonly<Record<string, string>>,
  excluded: Readonly<Record<string, string>>,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(labels).filter(([key]) => excluded[key] === undefined),
  );
}

function formatLabels(
  labels: Readonly<Record<string, string>>,
  fallback: string,
): string {
  const entries = Object.entries(labels).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  if (entries.length === 0) return fallback;
  return `{${entries
    .map(([key, value]) => `${key}="${value}"`)
    .join(', ')}}`;
}

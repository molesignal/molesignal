import {
  formatTimeSeriesAxisTimestamp,
  formatTimeSeriesTimestamp,
} from '@/viz/timeseries/formatters';

import { formatFieldValue } from '../../fieldConfig';
import type { DataField, DataFrame } from '../../schema';
import { stableValueKey, visualizationColor } from '../shared/colors';
import {
  medianPositiveStep,
  normalizedTimelinePositions,
} from '../shared/time';

export interface StateSegment {
  id: string;
  start: number;
  end: number;
  text: string;
  color: string;
}

export interface StateTimelineRow {
  id: string;
  name: string;
  segments: StateSegment[];
}

export interface StateLegendItem {
  id: string;
  text: string;
  color: string;
}

export interface StateTimelineModel {
  rows: StateTimelineRow[];
  start: number;
  end: number;
  usesTime: boolean;
  startLabel: string;
  endLabel: string;
  legend: StateLegendItem[];
  legendTruncated: boolean;
}

interface RowDraft {
  frame: DataFrame;
  field: DataField;
  positions: number[];
  usesTime: boolean;
}

export function prepareStateTimeline(
  frames: readonly DataFrame[],
  mergeEqual: boolean,
): StateTimelineModel | null {
  const drafts = frames.flatMap((frame) => {
    const time = frame.fields.find((field) => field.type === 'time');
    return frame.fields
      .filter((field) => field.type !== 'time' && field.values.length > 0)
      .map((field): RowDraft => {
        const axis = normalizedTimelinePositions(time?.values, field.values.length);
        return { frame, field, positions: axis.values, usesTime: axis.usesTime };
      });
  });
  if (drafts.length === 0) return null;

  const usesTime = drafts.every((draft) => draft.usesTime);
  if (!usesTime) {
    for (const draft of drafts) {
      draft.positions = Array.from(
        { length: draft.field.values.length },
        (_, index) => index,
      );
    }
  }

  const rows = drafts.map((draft) => buildRow(draft, mergeEqual));
  const segments = rows.flatMap((row) => row.segments);
  if (segments.length === 0) return null;
  const start = Math.min(...segments.map((segment) => segment.start));
  let end = Math.max(...segments.map((segment) => segment.end));
  if (end <= start) end = start + 1;
  const legendMap = new Map<string, StateLegendItem>();
  for (const segment of segments) {
    const id = `${segment.text}\u0000${segment.color}`;
    if (!legendMap.has(id)) {
      legendMap.set(id, { id, text: segment.text, color: segment.color });
    }
  }
  const allLegend = [...legendMap.values()];
  const span = end - start;

  return {
    rows,
    start,
    end,
    usesTime,
    startLabel: usesTime
      ? formatTimeSeriesAxisTimestamp(start, span)
      : formatIndex(start),
    endLabel: usesTime ? formatTimeSeriesAxisTimestamp(end, span) : formatIndex(end),
    legend: allLegend.slice(0, 8),
    legendTruncated: allLegend.length > 8,
  };
}

function buildRow(draft: RowDraft, mergeEqual: boolean): StateTimelineRow {
  const step = medianPositiveStep(draft.positions);
  const rawSegments = draft.field.values.map((raw, index): StateSegment => {
    const start = draft.positions[index] ?? index;
    const next = draft.positions[index + 1];
    const end = next !== undefined && next > start ? next : start + step;
    const display = formatFieldValue(raw, draft.field.config);
    const stateKey = stableValueKey(raw);
    return {
      id: `${draft.frame.refId}:${draft.field.id}:${index}`,
      start,
      end,
      text: display.text,
      color:
        display.color ??
        draft.field.config?.color?.value ??
        visualizationColor(stateKey),
    };
  });
  const segments = mergeEqual ? mergeStateSegments(rawSegments) : rawSegments;
  return {
    id: `${draft.frame.refId}:${draft.field.id}`,
    name:
      draft.field.config?.displayName ??
      (draft.frame.name
        ? `${draft.frame.name} · ${draft.field.name}`
        : draft.field.name),
    segments,
  };
}

export function mergeStateSegments(
  segments: readonly StateSegment[],
): StateSegment[] {
  const output: StateSegment[] = [];
  for (const segment of segments) {
    const previous = output.at(-1);
    if (
      previous &&
      previous.text === segment.text &&
      previous.color === segment.color &&
      previous.end === segment.start
    ) {
      previous.end = segment.end;
    } else {
      output.push({ ...segment });
    }
  }
  return output;
}

export function formatSegmentBoundary(value: number, model: StateTimelineModel): string {
  return model.usesTime ? formatTimeSeriesTimestamp(value, true) : formatIndex(value);
}

function formatIndex(value: number): string {
  return Number(value.toFixed(2)).toString();
}

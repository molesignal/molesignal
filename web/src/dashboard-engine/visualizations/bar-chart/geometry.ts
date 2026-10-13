import type { BarChartModel } from './model';

export interface BarRect {
  key: string;
  x: number;
  y: number;
  width: number;
  height: number;
  valueX: number;
  valueY: number;
  valueAnchor: 'start' | 'middle' | 'end';
  category: string;
  series: string;
  value: number;
  text: string;
  color: string;
}

export interface AxisLabel {
  key: string;
  x: number;
  y: number;
  text: string;
  anchor: 'start' | 'middle' | 'end';
}

export interface AxisLine {
  key: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface BarChartGeometry {
  rects: BarRect[];
  categoryLabels: AxisLabel[];
  valueTicks: AxisLabel[];
  /** A faint line across the plot at every mark of the value axis but zero. */
  gridLines: AxisLine[];
  zeroLine: { x1: number; y1: number; x2: number; y2: number };
  showValues: boolean;
}

/**
 * The widest a bar is drawn. A few categories in a wide panel are still bars
 * standing on an axis, with air between them, and not slabs that fill it.
 */
const MAX_BAR_SIZE = 56;
/** As many marks as a value axis carries. */
const MAX_VALUE_TICKS = 5;
/** What a character of an axis label takes, for a label to be cut to the room it has. */
const LABEL_CHARACTER_WIDTH = 6.5;

export function buildBarChartGeometry(
  model: BarChartModel,
  width: number,
  height: number,
  orientation: 'horizontal' | 'vertical',
  groupWidth: number,
  valueMode: 'auto' | 'always' | 'never',
): BarChartGeometry {
  const legendHeight = model.series.length > 1 && model.series.length <= 8 ? 22 : 0;
  return orientation === 'horizontal'
    ? horizontalGeometry(model, width, height, legendHeight, groupWidth, valueMode)
    : verticalGeometry(model, width, height, legendHeight, groupWidth, valueMode);
}

function verticalGeometry(
  model: BarChartModel,
  width: number,
  height: number,
  legendHeight: number,
  groupWidth: number,
  valueMode: 'auto' | 'always' | 'never',
): BarChartGeometry {
  const plot = { left: 48, right: Math.max(56, width - 12), top: 10 + legendHeight, bottom: Math.max(30, height - 30) };
  const plotWidth = Math.max(1, plot.right - plot.left);
  const plotHeight = Math.max(1, plot.bottom - plot.top);
  const step = plotWidth / Math.max(1, model.categories.length);
  const group = Math.min(step * clamp(groupWidth, 0.1, 1), MAX_BAR_SIZE * model.series.length);
  const barWidth = Math.max(0.5, group / model.series.length);
  const scale = valueScale(model);
  const y = (value: number) => plot.bottom - ((value - scale.min) / (scale.max - scale.min)) * plotHeight;
  const zero = y(0);
  const rects: BarRect[] = [];
  model.categories.forEach((category, categoryIndex) => {
    model.series.forEach((series, seriesIndex) => {
      const point = category.values[series.id];
      if (!point) return;
      const valueY = y(point.value);
      const x = plot.left + categoryIndex * step + (step - group) / 2 + seriesIndex * barWidth;
      rects.push({
        key: `${category.id}:${series.id}`,
        x,
        y: Math.min(zero, valueY),
        width: Math.max(0.5, barWidth - 1),
        height: Math.max(1, Math.abs(zero - valueY)),
        valueX: x + barWidth / 2,
        valueY: point.value >= 0 ? valueY - 4 : valueY + 11,
        valueAnchor: 'middle',
        category: category.tooltipLabel ?? category.label,
        series: series.name,
        value: point.value,
        text: point.text,
        color: point.color,
      });
    });
  });
  return {
    rects,
    categoryLabels: model.categories.map((category, index) => ({
      key: category.id,
      x: plot.left + (index + 0.5) * step,
      y: height - 10,
      text: shortLabel(category.label, step / LABEL_CHARACTER_WIDTH),
      anchor: 'middle',
    })),
    valueTicks: scale.ticks.map((value) => ({
      key: String(value),
      x: plot.left - 6,
      y: y(value) + 4,
      text: compact(value),
      anchor: 'end',
    })),
    gridLines: scale.ticks
      .filter((value) => value !== 0)
      .map((value) => ({ key: String(value), x1: plot.left, y1: y(value), x2: plot.right, y2: y(value) })),
    zeroLine: { x1: plot.left, y1: zero, x2: plot.right, y2: zero },
    showValues: shouldShowValues(valueMode, rects.length, barWidth, 24),
  };
}

function horizontalGeometry(
  model: BarChartModel,
  width: number,
  height: number,
  legendHeight: number,
  groupWidth: number,
  valueMode: 'auto' | 'always' | 'never',
): BarChartGeometry {
  const plot = { left: Math.min(104, Math.max(72, width * 0.24)), right: Math.max(80, width - 14), top: 10 + legendHeight, bottom: Math.max(32, height - 24) };
  const plotWidth = Math.max(1, plot.right - plot.left);
  const plotHeight = Math.max(1, plot.bottom - plot.top);
  const step = plotHeight / Math.max(1, model.categories.length);
  const group = Math.min(step * clamp(groupWidth, 0.1, 1), MAX_BAR_SIZE * model.series.length);
  const barHeight = Math.max(0.5, group / model.series.length);
  const scale = valueScale(model);
  const x = (value: number) => plot.left + ((value - scale.min) / (scale.max - scale.min)) * plotWidth;
  const zero = x(0);
  const rects: BarRect[] = [];
  model.categories.forEach((category, categoryIndex) => {
    model.series.forEach((series, seriesIndex) => {
      const point = category.values[series.id];
      if (!point) return;
      const valueX = x(point.value);
      const y = plot.top + categoryIndex * step + (step - group) / 2 + seriesIndex * barHeight;
      rects.push({
        key: `${category.id}:${series.id}`,
        x: Math.min(zero, valueX),
        y,
        width: Math.max(1, Math.abs(zero - valueX)),
        height: Math.max(0.5, barHeight - 1),
        valueX: point.value >= 0 ? valueX + 4 : valueX - 4,
        valueY: y + barHeight / 2 + 3,
        valueAnchor: point.value >= 0 ? 'start' : 'end',
        category: category.tooltipLabel ?? category.label,
        series: series.name,
        value: point.value,
        text: point.text,
        color: point.color,
      });
    });
  });
  return {
    rects,
    categoryLabels: model.categories.map((category, index) => ({
      key: category.id,
      x: plot.left - 7,
      y: plot.top + (index + 0.5) * step + 4,
      text: shortLabel(category.label, (plot.left - 12) / LABEL_CHARACTER_WIDTH),
      anchor: 'end',
    })),
    valueTicks: scale.ticks.map((value) => ({
      key: String(value),
      x: x(value),
      y: height - 7,
      text: compact(value),
      anchor: 'middle',
    })),
    gridLines: scale.ticks
      .filter((value) => value !== 0)
      .map((value) => ({ key: String(value), x1: x(value), y1: plot.top, x2: x(value), y2: plot.bottom })),
    zeroLine: { x1: zero, y1: plot.top, x2: zero, y2: plot.bottom },
    showValues: shouldShowValues(valueMode, rects.length, barHeight, 12),
  };
}

/**
 * The scale the values are read against: marks a round step apart (1, 2, 2.5,
 * 5 and their tenfolds), few enough to read, and ends that lie on marks, so no
 * bar runs past the last line. Zero is always a mark: the range of a bar chart
 * includes it.
 */
export function valueScale(model: Pick<BarChartModel, 'range'>): { min: number; max: number; ticks: number[] } {
  const { min, max } = model.range;
  const span = max - min;
  const magnitude = 10 ** Math.floor(Math.log10(span / (MAX_VALUE_TICKS - 1)));
  const step =
    [1, 2, 2.5, 5, 10].map((count) => count * magnitude).find((candidate) => span / candidate <= MAX_VALUE_TICKS - 1) ??
    10 * magnitude;
  const first = Math.floor(min / step);
  const last = Math.ceil(max / step);

  return {
    min: first * step,
    max: last * step,
    // Counted in whole steps, so a mark is a round number and not a sum of rounding errors.
    ticks: Array.from({ length: last - first + 1 }, (_, index) => Number(((first + index) * step).toPrecision(12))),
  };
}

/** Whether each bar says its value: where it is asked for, or where the bars are few and have room for one. */
function shouldShowValues(mode: string, count: number, barSize: number, least: number): boolean {
  return mode === 'always' || (mode === 'auto' && count <= 16 && barSize >= least);
}

/** A label cut to the room its category has: never to fewer characters than tell it from its neighbours. */
function shortLabel(value: string, room: number): string {
  const most = Math.max(6, Math.floor(room));

  return value.length > most ? `${value.slice(0, most - 1)}…` : value;
}

function compact(value: number): string {
  return Math.abs(value) >= 1_000 ? value.toLocaleString(undefined, { notation: 'compact', maximumFractionDigits: 1 }) : Number(value.toFixed(2)).toString();
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

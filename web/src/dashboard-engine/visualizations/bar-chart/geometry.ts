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

export interface BarChartGeometry {
  rects: BarRect[];
  categoryLabels: AxisLabel[];
  valueTicks: AxisLabel[];
  zeroLine: { x1: number; y1: number; x2: number; y2: number };
  showValues: boolean;
}

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
  const step = plotWidth / model.categories.length;
  const group = step * clamp(groupWidth, 0.1, 1);
  const barWidth = Math.max(0.5, group / model.series.length);
  const y = (value: number) =>
    plot.bottom - ((value - model.range.min) / (model.range.max - model.range.min)) * plotHeight;
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
        category: category.label,
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
      text: shortLabel(category.label),
      anchor: 'middle',
    })),
    valueTicks: valueTicks(model).map((value) => ({
      key: String(value),
      x: plot.left - 6,
      y: y(value) + 3,
      text: compact(value),
      anchor: 'end',
    })),
    zeroLine: { x1: plot.left, y1: zero, x2: plot.right, y2: zero },
    showValues: shouldShowValues(valueMode, rects.length, barWidth),
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
  const step = plotHeight / model.categories.length;
  const group = step * clamp(groupWidth, 0.1, 1);
  const barHeight = Math.max(0.5, group / model.series.length);
  const x = (value: number) =>
    plot.left + ((value - model.range.min) / (model.range.max - model.range.min)) * plotWidth;
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
        category: category.label,
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
      y: plot.top + (index + 0.5) * step + 3,
      text: shortLabel(category.label),
      anchor: 'end',
    })),
    valueTicks: valueTicks(model).map((value) => ({
      key: String(value),
      x: x(value),
      y: height - 7,
      text: compact(value),
      anchor: 'middle',
    })),
    zeroLine: { x1: zero, y1: plot.top, x2: zero, y2: plot.bottom },
    showValues: shouldShowValues(valueMode, rects.length, barHeight),
  };
}

function valueTicks(model: BarChartModel): number[] {
  return [...new Set([model.range.min, 0, model.range.max])].sort((a, b) => a - b);
}

function shouldShowValues(mode: string, count: number, barSize: number): boolean {
  return mode === 'always' || (mode === 'auto' && count <= 16 && barSize >= 10);
}

function shortLabel(value: string): string {
  return value.length > 14 ? `${value.slice(0, 13)}…` : value;
}

function compact(value: number): string {
  return Math.abs(value) >= 1_000 ? value.toLocaleString(undefined, { notation: 'compact', maximumFractionDigits: 1 }) : Number(value.toFixed(2)).toString();
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

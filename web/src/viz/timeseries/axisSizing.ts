import type uPlot from 'uplot';

export function buildYAxisSize(
  font: string,
  compact: boolean,
): Exclude<uPlot.Axis.Size, number> {
  const minimum = compact ? 58 : 68;
  const tickAndGap = compact ? 9 : 12;
  return (plot, values) => {
    if (!values?.length) return minimum;
    const context = plot.ctx;
    context.save();
    try {
      context.font = font;
      let widestValue = 0;
      for (const value of values) {
        for (const line of String(value ?? '').split('\n')) {
          widestValue = Math.max(widestValue, context.measureText(line).width);
        }
      }
      return Math.max(minimum, Math.ceil(widestValue + tickAndGap + 3));
    } finally {
      context.restore();
    }
  };
}

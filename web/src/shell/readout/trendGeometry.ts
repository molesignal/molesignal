/** The drawing of a trend in a box: a line, the area under it, and where it ends. */
export interface SparklineGeometry {
  line: string;
  area: string;
  /** The last point, in percent of the box, for the dot that marks "now". */
  end: { x: number; y: number };
}

export interface SparklineBar {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const SPARKLINE_WIDTH = 100;
export const SPARKLINE_HEIGHT = 32;

// The stroke is 1.5px; a point on the very edge would be cut in half.
const INSET = 2;

type Sample = number | null | undefined;

function finite(values: readonly Sample[]): number[] {
  return values.filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
}

function scale(values: readonly number[], zeroBased: boolean): { min: number; max: number } {
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (zeroBased) min = Math.min(0, min);
  if (min === max) {
    // A flat series has no shape to scale to. Zero sits on the baseline (nothing
    // is happening); any other level sits in the middle.
    if (max === 0) max = 1;
    else {
      min -= Math.abs(min) || 1;
      max += Math.abs(max) || 1;
    }
  }
  return { min, max };
}

function round(value: number): number {
  return Number(value.toFixed(2));
}

/**
 * The path of a trend line. Gaps in the data are skipped, not drawn as zero:
 * the line joins the samples on either side.
 */
export function sparklineGeometry(
  values: readonly Sample[],
  { zeroBased = false }: { zeroBased?: boolean } = {},
): SparklineGeometry | null {
  const samples = finite(values);
  if (samples.length < 2) return null;
  const { min, max } = scale(samples, zeroBased);
  const drawable = SPARKLINE_HEIGHT - INSET * 2;

  const points = samples.map((value, index) => ({
    x: round((index / (samples.length - 1)) * SPARKLINE_WIDTH),
    y: round(SPARKLINE_HEIGHT - INSET - ((value - min) / (max - min)) * drawable),
  }));
  const line = `M ${points.map((point) => `${point.x},${point.y}`).join(' L ')}`;
  const last = points[points.length - 1]!;

  return {
    line,
    area: `${line} L ${SPARKLINE_WIDTH},${SPARKLINE_HEIGHT} L 0,${SPARKLINE_HEIGHT} Z`,
    end: {
      x: round((last.x / SPARKLINE_WIDTH) * 100),
      y: round((last.y / SPARKLINE_HEIGHT) * 100),
    },
  };
}

/** The bars of a trend drawn as counts: one per sample, growing from the baseline. */
export function sparklineBars(values: readonly Sample[]): SparklineBar[] {
  const samples = values.map((value) =>
    typeof value === 'number' && Number.isFinite(value) ? Math.max(0, value) : 0,
  );
  if (samples.length === 0) return [];
  const max = Math.max(...samples);
  const slot = SPARKLINE_WIDTH / samples.length;
  const width = round(Math.max(slot * 0.68, 0.6));

  return samples.map((value, index) => {
    // A bucket with something in it is always visible, however small beside the peak.
    const height = max === 0 ? 0 : round(Math.max((value / max) * SPARKLINE_HEIGHT, value > 0 ? 1.5 : 0));
    return {
      x: round(index * slot + (slot - width) / 2),
      y: round(SPARKLINE_HEIGHT - height),
      width,
      height,
    };
  });
}

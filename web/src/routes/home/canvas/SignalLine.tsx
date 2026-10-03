import { cn } from '@/shell/lib/cn';

const WIDTH = 400;
const HEIGHT = 48;
const MID = HEIGHT / 2;
const PAD = 6;

interface Point {
  x: number;
  y: number;
}

/** Horizontal-tangent Béziers: smooth, never overshoots the data. */
function smoothPath(points: readonly Point[]): string {
  const [first, ...rest] = points;
  if (!first) return '';
  let d = `M${first.x.toFixed(1)} ${first.y.toFixed(1)}`;
  let prev = first;
  for (const point of rest) {
    const mid = (point.x - prev.x) / 2;
    d += ` C${(prev.x + mid).toFixed(1)} ${prev.y.toFixed(1)} ${(point.x - mid).toFixed(1)} ${point.y.toFixed(1)} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`;
    prev = point;
  }
  return d;
}

/** Scales a series into the viewBox; a flat or empty series stays a flat line. */
export function signalPoints(values: readonly number[]): Point[] | null {
  if (values.length < 2) return null;
  const max = Math.max(...values);
  if (!(max > 0)) return null;
  const step = WIDTH / (values.length - 1);
  return values.map((value, index) => ({
    x: index * step,
    y: HEIGHT - PAD - (Math.max(0, value) / max) * (HEIGHT - PAD * 2),
  }));
}

/**
 * The product mark as a living graphic: a pulse line. With data it traces the
 * intake trend; with none it is a straight line that ends where the signal
 * stopped, so the emptiest state is also the most recognizable one.
 *
 * Decorative: it carries no information the surrounding copy does not.
 * Color comes from `--tone` set by the parent.
 */
export function SignalLine({
  values,
  className,
}: {
  values?: readonly number[] | undefined;
  className?: string | undefined;
}) {
  const points = values ? signalPoints(values) : null;

  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      className={cn('overflow-visible', className)}
      style={{ color: 'var(--tone, var(--tx-3))' }}
    >
      {points ? (
        <>
          <path
            d={`${smoothPath(points)} L${WIDTH} ${HEIGHT} L0 ${HEIGHT} Z`}
            fill="currentColor"
            opacity="0.08"
          />
          <path
            d={smoothPath(points)}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.75"
            strokeLinecap="round"
            strokeLinejoin="round"
            vectorEffect="non-scaling-stroke"
          />
        </>
      ) : (
        <>
          <path
            d={`M0 ${MID} H${WIDTH * 0.78}`}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.75"
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
          <path
            d={`M${WIDTH * 0.78 + 14} ${MID} H${WIDTH}`}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.75"
            strokeLinecap="round"
            strokeDasharray="1 7"
            opacity="0.55"
            vectorEffect="non-scaling-stroke"
          />
        </>
      )}
    </svg>
  );
}

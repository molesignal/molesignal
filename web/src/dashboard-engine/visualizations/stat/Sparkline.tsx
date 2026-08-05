/*
 * Geometry principles adapted from Grafana UI v13.1.0 Sparkline.
 * Copyright 2015 Grafana Labs. Licensed under Apache License 2.0.
 * Modified for MoleSignal's dependency-free responsive SVG renderer.
 */

const WIDTH = 100;
const HEIGHT = 32;

export interface SparklineGeometry {
  line: string;
  area: string;
}

export function Sparkline({
  values,
  color,
  className,
}: {
  values: readonly number[];
  color: string;
  className?: string;
}) {
  const geometry = sparklineGeometry(values);
  if (!geometry) return null;
  return (
    <svg
      aria-hidden="true"
      className={className}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
    >
      <path d={geometry.area} fill={color} opacity="0.12" />
      <path
        d={geometry.line}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

export function sparklineGeometry(
  values: readonly number[],
): SparklineGeometry | null {
  const finite = values.filter(Number.isFinite);
  if (finite.length < 2) return null;
  let min = Math.min(...finite);
  let max = Math.max(...finite);
  if (min === max) {
    min -= 1;
    max += 1;
  }
  const points = finite.map((value, index) => {
    const x = (index / (finite.length - 1)) * WIDTH;
    const y = HEIGHT - ((value - min) / (max - min)) * (HEIGHT - 2) - 1;
    return `${round(x)},${round(y)}`;
  });
  return {
    line: `M ${points.join(' L ')}`,
    area: `M 0,${HEIGHT} L ${points.join(' L ')} L ${WIDTH},${HEIGHT} Z`,
  };
}

function round(value: number): number {
  return Number(value.toFixed(2));
}

import * as React from 'react';

import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

/**
 * Storybook-lite demo to validate uPlot performance with large series.
 * Mount this on a scratch route to eyeball downsampling + theme + brush.
 */
export function TimeSeriesChartDemo() {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const n = Math.max(
    1,
    Math.min(20_000_000, parseInt(params.get('n') ?? '1000000', 10) || 1_000_000),
  );
  const [timestamps, p50, p95, p99] = React.useMemo(() => generateSeries(n), [n]);
  return (
    <div className="p-5">
      <div className="mb-2 text-xs text-muted-foreground">
        {n.toLocaleString()} points, downsampled to chartWidth × 3. Drag to brush;
        horizontal scroll zooms continuously; drag the x-axis to pan;
        Cmd/Ctrl + wheel zooms around the pointer.
      </div>
      <TimeSeriesChart
        series={[
          { id: 'p50', name: 'p50', data: p50, timestamps },
          { id: 'p95', name: 'p95', data: p95, timestamps },
          { id: 'p99', name: 'p99', data: p99, timestamps },
        ]}
        height={320}
        onRangeSelect={(r) => console.log('range', r)}
        onPan={(d) => console.log('pan', d)}
      />
    </div>
  );
}

function generateSeries(n: number): [number[], number[], number[], number[]] {
  const xs = new Array<number>(n);
  const y1 = new Array<number>(n);
  const y2 = new Array<number>(n);
  const y3 = new Array<number>(n);
  // Deterministic pseudo-noise (LCG) — needed for focus-ring visual
  // baselines: `Math.random` makes the canvas pixels unstable across runs
  // and pinches snapshot tests despite the focus ring being byte-stable.
  let lcg = 0x12345678;
  const rand = () => {
    lcg = (lcg * 1103515245 + 12345) & 0x7fffffff;
    return lcg / 0x7fffffff;
  };
  const start = 1779897600 - n; // 2026-05-23T10:00:00Z anchor, stable across runs
  for (let i = 0; i < n; i++) {
    xs[i] = start + i;
    const trend = Math.sin(i / 30000) * 40 + 60;
    const noise = rand() * 10;
    y1[i] = trend + noise * 0.5;
    y2[i] = trend + noise * 1.4 + 12;
    y3[i] = trend + noise * 2.0 + 30;
  }
  return [xs, y1, y2, y3];
}

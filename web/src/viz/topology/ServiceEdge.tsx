import * as React from 'react';
import { BaseEdge, EdgeLabelRenderer, getBezierPath, useViewport, type EdgeProps } from 'reactflow';

export interface ServiceEdgeData {
  rps: number;
  err_rate: number;
  p95_ms: number;
  showLabel?: boolean;
}

const ROTATE_MS = 3000;
const LABELS: Array<'rps' | 'err' | 'p95'> = ['rps', 'err', 'p95'];

export function ServiceEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
  style,
  markerEnd,
}: EdgeProps<ServiceEdgeData>) {
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });

  const viewport = useViewport();
  const isOnscreen = isLabelOnScreen(labelX, labelY, viewport);
  const [idx, setIdx] = React.useState(0);
  const [hover, setHover] = React.useState(false);

  React.useEffect(() => {
    if (!isOnscreen || hover) return;
    const handle = globalThis.setInterval(() => setIdx((i) => (i + 1) % LABELS.length), ROTATE_MS);
    return () => globalThis.clearInterval(handle);
  }, [isOnscreen, hover]);

  if (!data || data.showLabel === false) {
    return <BaseEdge id={id} path={edgePath} markerEnd={markerEnd ?? ''} style={style ?? {}} />;
  }

  const labelText = formatLabel(LABELS[idx]!, data);

  return (
    <>
      <BaseEdge id={id} path={edgePath} markerEnd={markerEnd ?? ''} style={style ?? {}} />
      <EdgeLabelRenderer>
        <div
          className="nodrag nopan absolute rounded-sm border border-border bg-surface px-1.5 py-0.5 font-sans text-xs text-foreground shadow-sm"
          style={{ transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`, pointerEvents: 'all' }}
          onMouseEnter={() => setHover(true)}
          onMouseLeave={() => setHover(false)}
        >
          {hover ? (
            <div className="flex flex-col gap-0.5 leading-tight">
              <span>{formatLabel('rps', data)}</span>
              <span>{formatLabel('err', data)}</span>
              <span>{formatLabel('p95', data)}</span>
            </div>
          ) : (
            labelText
          )}
        </div>
      </EdgeLabelRenderer>
    </>
  );
}

function formatLabel(kind: 'rps' | 'err' | 'p95', data: ServiceEdgeData): string {
  switch (kind) {
    case 'rps':
      return data.rps >= 1000 ? `${(data.rps / 1000).toFixed(1)}k rps` : `${data.rps.toFixed(0)} rps`;
    case 'err':
      return `${(data.err_rate * 100).toFixed(2)}% err`;
    case 'p95':
      return `${data.p95_ms.toFixed(0)}ms p95`;
  }
}

function isLabelOnScreen(labelX: number, labelY: number, viewport: { x: number; y: number; zoom: number }): boolean {
  if (typeof window === 'undefined') return true;
  const screenX = labelX * viewport.zoom + viewport.x;
  const screenY = labelY * viewport.zoom + viewport.y;
  const halo = 0.2;
  const w = window.innerWidth;
  const h = window.innerHeight;
  return screenX > -w * halo && screenX < w * (1 + halo) && screenY > -h * halo && screenY < h * (1 + halo);
}

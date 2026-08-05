import * as React from 'react';

/*
 * Small SVG renderers that are not time-series visualizations. Time-series
 * data is rendered exclusively by viz/timeseries/TimeSeriesChart (Canvas).
 */

function seededRandom(seed: number) {
  let value = seed;
  return () => {
    value = (value * 9301 + 49297) % 233280;
    return value / 233280;
  };
}

export function BarChart({
  data,
  color = 'var(--chart-7)',
  height = 100,
  labels,
}: {
  data: number[];
  color?: string;
  height?: number;
  labels?: string[];
}) {
  if (data.length === 0) return null;
  const width = 400;
  const padding = { left: 4, right: 4, top: 6, bottom: labels ? 16 : 4 };
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;
  const max = Math.max(...data, 1);
  const barWidth = chartWidth / data.length;
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      style={{ height }}
      className="block w-full"
    >
      {data.map((value, index) => (
        <rect
          key={index}
          x={padding.left + index * barWidth + 1}
          y={padding.top + chartHeight - (value / max) * chartHeight}
          width={Math.max(barWidth - 2, 1)}
          height={(value / max) * chartHeight}
          fill={color}
          opacity={0.85}
          rx={1}
        />
      ))}
      {labels && (
        <g fontFamily="var(--font-sans)" fontSize="9" fill="var(--tx-3)">
          {labels.map((label, index) => (
            <text
              key={label}
              x={padding.left + index * barWidth + barWidth / 2}
              y={height - 3}
              textAnchor="middle"
            >
              {label}
            </text>
          ))}
        </g>
      )}
    </svg>
  );
}

export function Heatmap({
  rows = 7,
  cols = 24,
  seed = 42,
  color = 'var(--chart-7)',
}: {
  rows?: number;
  cols?: number;
  seed?: number;
  color?: string;
}) {
  const data = React.useMemo(() => {
    const random = seededRandom(seed);
    return Array.from({ length: rows }, () =>
      Array.from({ length: cols }, () => random()),
    );
  }, [rows, cols, seed]);
  const width = 600;
  const height = 140;
  const cellWidth = width / cols;
  const cellHeight = height / rows;
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className="block w-full"
    >
      {data.map((row, rowIndex) =>
        row.map((value, columnIndex) => (
          <rect
            key={`${rowIndex}-${columnIndex}`}
            x={columnIndex * cellWidth + 0.5}
            y={rowIndex * cellHeight + 0.5}
            width={cellWidth - 1}
            height={cellHeight - 1}
            fill={color}
            opacity={0.08 + value * 0.85}
            rx={1}
          />
        )),
      )}
    </svg>
  );
}

export function Donut({
  value = 75,
  max = 100,
  color = 'var(--green)',
  size = 64,
}: {
  value?: number;
  max?: number;
  color?: string;
  size?: number;
}) {
  const radius = size / 2 - 6;
  const circumference = 2 * Math.PI * radius;
  const percentage = value / max;
  return (
    <svg viewBox={`0 0 ${size} ${size}`} style={{ width: size, height: size }}>
      <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke="var(--bd-1)"
        strokeWidth={4}
      />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke={color}
        strokeWidth={4}
        strokeDasharray={`${circumference * percentage} ${circumference}`}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
        strokeLinecap="round"
      />
      <text
        x={size / 2}
        y={size / 2 + 4}
        textAnchor="middle"
        fontFamily="var(--font-sans)"
        fontSize="12"
        fontWeight={600}
        fill="var(--tx-0)"
      >
        {Math.round(percentage * 100)}%
      </text>
    </svg>
  );
}

export interface ServiceNode {
  id: string;
  short: string;
  name: string;
  qps?: number;
  x: number;
  y: number;
  status?: 'healthy' | 'degraded' | 'error' | 'unknown';
}

export interface ServiceEdge {
  from: string;
  to: string;
  label?: string;
}

export function ServiceMap({
  nodes,
  edges,
  height = 360,
  className,
}: {
  nodes: ServiceNode[];
  edges: ServiceEdge[];
  height?: number;
  className?: string;
}) {
  const colors = {
    healthy: 'var(--green)',
    degraded: 'var(--yellow)',
    error: 'var(--red)',
    unknown: 'var(--tx-2)',
  };
  return (
    <svg viewBox={`0 0 800 ${height}`} className={`block w-full ${className ?? ''}`}>
      <defs>
        <marker
          id="arrow"
          viewBox="0 0 10 10"
          refX="9"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--bd-2)" />
        </marker>
      </defs>
      {edges.map((edge, index) => {
        const from = nodes.find((node) => node.id === edge.from);
        const to = nodes.find((node) => node.id === edge.to);
        if (!from || !to) return null;
        return (
          <g key={`${edge.from}-${edge.to}-${index}`}>
            <line
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke="var(--bd-2)"
              strokeWidth={1}
              markerEnd="url(#arrow)"
            />
            {edge.label && (
              <text
                x={(from.x + to.x) / 2}
                y={(from.y + to.y) / 2 - 4}
                textAnchor="middle"
                fontSize="9"
                fill="var(--tx-3)"
                fontFamily="var(--font-sans)"
              >
                {edge.label}
              </text>
            )}
          </g>
        );
      })}
      {nodes.map((node) => (
        <g key={node.id} transform={`translate(${node.x}, ${node.y})`}>
          <circle
            r={22}
            fill="var(--bg-2)"
            stroke={colors[node.status ?? 'healthy']}
            strokeWidth={1.5}
          />
          <circle
            r={3}
            cx={14}
            cy={-14}
            fill={colors[node.status ?? 'healthy']}
          />
          <text
            textAnchor="middle"
            y={3}
            fontSize="10"
            fontWeight={600}
            fill="var(--tx-0)"
            fontFamily="var(--font-sans)"
          >
            {node.short}
          </text>
          <text
            textAnchor="middle"
            y={38}
            fontSize="9"
            fill="var(--tx-2)"
            fontFamily="var(--font-sans)"
          >
            {node.name}
          </text>
          {node.qps !== undefined && (
            <text
              textAnchor="middle"
              y={50}
              fontSize="8"
              fill="var(--tx-3)"
              fontFamily="var(--font-sans)"
            >
              {node.qps} req/s
            </text>
          )}
        </g>
      ))}
    </svg>
  );
}

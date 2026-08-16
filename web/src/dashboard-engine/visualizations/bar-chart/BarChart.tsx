import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { buildBarChartGeometry } from './geometry';
import type { BarChartModel } from './model';
import { useElementSize } from '../shared/MeasuredContainer';

export function BarChart({
  model,
  height,
  orientation,
  groupWidth,
  minCategoryWidth = 18,
  showValues,
}: {
  model: BarChartModel;
  height: number;
  orientation: 'horizontal' | 'vertical';
  groupWidth: number;
  minCategoryWidth?: number;
  showValues: 'auto' | 'always' | 'never';
}) {
  const [ref, size] = useElementSize({ width: 480, height });
  const chartWidth =
    orientation === 'vertical'
      ? Math.max(160, size.width, model.categories.length * minCategoryWidth + 64)
      : Math.max(160, size.width);
  const chartHeight =
    orientation === 'horizontal'
      ? Math.max(96, size.height, model.categories.length * 18 + 48)
      : Math.max(96, size.height);
  const geometry = buildBarChartGeometry(
    model,
    chartWidth,
    chartHeight,
    orientation,
    groupWidth,
    showValues,
  );
  const showLegend = model.series.length > 1 && model.series.length <= 8;

  return (
    <TooltipProvider delayDuration={0}>
      <div ref={ref} className="h-full min-h-24 w-full overflow-auto">
        <svg
        role="img"
        aria-label={`Bar chart with ${model.categories.length} categories and ${model.series.length} series`}
        className="block max-w-none overflow-visible font-sans"
        style={{ width: chartWidth, height: chartHeight }}
        viewBox={`0 0 ${chartWidth} ${chartHeight}`}
        preserveAspectRatio="none"
        fontFamily="var(--font-sans)"
      >
        <title>{`Bar chart with ${model.categories.length} categories and ${model.series.length} series`}</title>
        {showLegend && (
          <g aria-hidden="true">
            {model.series.map((series, index) => (
              <g key={series.id} transform={`translate(${12 + index * 92}, 12)`}>
                <rect width="8" height="8" rx="1" fill={series.color} />
                <text x="12" y="8" fontSize="9" fill="var(--tx-3)">
                  {series.name.length > 11 ? `${series.name.slice(0, 10)}…` : series.name}
                </text>
              </g>
            ))}
          </g>
        )}
        <line
          {...geometry.zeroLine}
          stroke="var(--bd-1)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        {geometry.categoryLabels.map((label) => (
          <text
            key={label.key}
            x={label.x}
            y={label.y}
            textAnchor={label.anchor}
            fontSize="9"
            fill="var(--tx-3)"
          >
            {label.text}
          </text>
        ))}
        {geometry.valueTicks.map((label) => (
          <text
            key={label.key}
            x={label.x}
            y={label.y}
            textAnchor={label.anchor}
            fontSize="9"
            fill="var(--tx-3)"
          >
            {label.text}
          </text>
        ))}
        {geometry.rects.map((rect) => (
          <g key={rect.key}>
            <Tooltip>
              <TooltipTrigger asChild>
                <rect
                  data-testid="bar-chart-bar"
                  aria-label={`${rect.category} · ${rect.series}: ${rect.text}`}
                  tabIndex={0}
                  className="cursor-default transition-opacity hover:opacity-80 focus-visible:opacity-80"
                  x={rect.x}
                  y={rect.y}
                  width={rect.width}
                  height={rect.height}
                  rx="1"
                  fill={rect.color}
                />
              </TooltipTrigger>
              <TooltipContent side="top" className="font-sans">
                <p className="font-strong text-tx-0">{rect.category}</p>
                <p className="text-tx-2">
                  {rect.series}: {rect.text}
                </p>
              </TooltipContent>
            </Tooltip>
            {geometry.showValues && (
              <text
                x={rect.valueX}
                y={rect.valueY}
                textAnchor={rect.valueAnchor}
                fontSize="8"
                fill="var(--tx-2)"
              >
                {rect.text}
              </text>
            )}
          </g>
        ))}
        </svg>
      </div>
    </TooltipProvider>
  );
}

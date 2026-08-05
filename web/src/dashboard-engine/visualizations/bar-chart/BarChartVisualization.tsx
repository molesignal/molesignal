import { BarChart } from './BarChart';
import { prepareBarChart } from './model';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import { calculationOption } from '../shared/reduction';
import type { VisualizationProps } from '../shared/types';

export function BarChartVisualization({
  data,
  options,
  height,
}: VisualizationProps) {
  const model = prepareBarChart(
    data.frames,
    calculationOption(options.calculation),
  );
  if (!model) return <EmptyVisualization />;
  return (
    <BarChart
      model={model}
      height={height}
      orientation={options.orientation === 'horizontal' ? 'horizontal' : 'vertical'}
      groupWidth={numberOption(options.groupWidth, 0.7)}
      showValues={showValueOption(options.showValues)}
    />
  );
}

function numberOption(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function showValueOption(value: unknown): 'auto' | 'always' | 'never' {
  return value === 'always' || value === 'never' ? value : 'auto';
}

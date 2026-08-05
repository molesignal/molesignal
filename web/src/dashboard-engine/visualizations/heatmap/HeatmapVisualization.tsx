import { Heatmap } from './Heatmap';
import { prepareHeatmap } from './model';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import type { VisualizationProps } from '../shared/types';

export function HeatmapVisualization({
  data,
  options,
  height,
}: VisualizationProps) {
  const model = prepareHeatmap(data.frames);
  if (!model) return <EmptyVisualization />;
  return (
    <Heatmap
      model={model}
      height={height}
      colorScheme={options.colorScheme}
    />
  );
}

import { prepareStateTimeline } from './model';
import { StateTimeline } from './StateTimeline';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import type { VisualizationProps } from '../shared/types';

export function StateTimelineVisualization({
  data,
  options,
  height,
}: VisualizationProps) {
  const model = prepareStateTimeline(data.frames, options.mergeEqual !== false);
  if (!model) return <EmptyVisualization />;
  return (
    <StateTimeline
      model={model}
      height={height}
      showValues={showValueOption(options.showValues)}
    />
  );
}

function showValueOption(value: unknown): 'auto' | 'always' | 'never' {
  return value === 'always' || value === 'never' ? value : 'auto';
}

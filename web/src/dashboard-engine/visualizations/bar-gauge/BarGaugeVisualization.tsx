import { HorizontalBarGauge, VerticalBarGauge } from './BarGauge';
import { prepareBarGaugeValues } from './model';
import { EmptyVisualization } from '../shared/EmptyVisualization';
import { calculationOption } from '../shared/reduction';
import type { VisualizationProps } from '../shared/types';

export function BarGaugeVisualization({
  data,
  options,
}: VisualizationProps) {
  const values = prepareBarGaugeValues(
    data.frames,
    calculationOption(options.calculation),
  );
  if (values.length === 0) return <EmptyVisualization />;

  const vertical = options.orientation === 'vertical';
  const displayMode = options.displayMode === 'thresholds' ? 'thresholds' : 'basic';
  const showThresholdMarkers = options.showThresholdMarkers !== false;

  if (vertical) {
    return (
      <div
        className="grid h-full min-h-0 gap-3 overflow-x-auto px-2 py-1"
        style={{ gridTemplateColumns: `repeat(${values.length}, minmax(3.5rem, 1fr))` }}
      >
        {values.map((item) => (
          <VerticalBarGauge
            key={item.key}
            item={item}
            displayMode={displayMode}
            showThresholdMarkers={showThresholdMarkers}
          />
        ))}
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col justify-center gap-2 overflow-auto px-2 py-1">
      {values.map((item) => (
        <HorizontalBarGauge
          key={item.key}
          item={item}
          displayMode={displayMode}
          showThresholdMarkers={showThresholdMarkers}
        />
      ))}
    </div>
  );
}

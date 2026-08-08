import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';

import type {
  TimeSeriesLegendMode,
  TimeSeriesLegendPlacement,
} from '../types';

export function LegendViewport({
  mode,
  placement,
  seriesIdentity = false,
  children,
}: {
  mode: Exclude<TimeSeriesLegendMode, 'hidden'>;
  placement: TimeSeriesLegendPlacement;
  seriesIdentity?: boolean;
  children: ReactNode;
}) {
  const adaptiveHeight = seriesIdentity && placement === 'bottom';

  return (
    <div
      className={cn(
        'min-h-0 shrink-0 bg-bg-1',
        adaptiveHeight ? 'overflow-visible' : 'overflow-auto',
        placement === 'right'
          ? 'h-full w-[40%] max-w-96 border-l border-bd-0'
          : adaptiveHeight
            ? 'border-t border-bd-0'
            : 'max-h-[35%]',
        mode === 'list' && 'p-1',
      )}
      data-adaptive-height={adaptiveHeight ? 'true' : 'false'}
      data-legend-mode={mode}
      data-legend-placement={placement}
      data-testid="time-series-legend"
    >
      {children}
    </div>
  );
}

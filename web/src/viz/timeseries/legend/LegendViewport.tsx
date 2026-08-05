import type { ReactNode } from 'react';

import { cn } from '@/shell/lib/cn';

import type {
  TimeSeriesLegendMode,
  TimeSeriesLegendPlacement,
} from '../types';

export function LegendViewport({
  mode,
  placement,
  children,
}: {
  mode: Exclude<TimeSeriesLegendMode, 'hidden'>;
  placement: TimeSeriesLegendPlacement;
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        'min-h-0 shrink-0 overflow-auto bg-bg-1',
        placement === 'right'
          ? 'h-full w-[40%] max-w-96 border-l border-bd-0'
          : 'max-h-[35%]',
        mode === 'list' && 'p-1',
      )}
      data-legend-mode={mode}
      data-legend-placement={placement}
      data-testid="time-series-legend"
    >
      {children}
    </div>
  );
}

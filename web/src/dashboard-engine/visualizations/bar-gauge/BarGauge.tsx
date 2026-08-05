/*
 * Layout principles adapted from Grafana UI v13.1.0 BarGauge.
 * Copyright 2015 Grafana Labs. Licensed under Apache License 2.0.
 * Modified for MoleSignal's local model, token system, and DOM meter semantics.
 */

import { cn } from '@/shell/lib/cn';

import type { BarGaugeValue } from './model';
import { valueRatio } from '../shared/range';

export function HorizontalBarGauge({
  item,
  displayMode,
  showThresholdMarkers,
}: BarGaugeProps) {
  return (
    <div className="grid min-w-0 grid-cols-[minmax(5rem,1fr)_minmax(7rem,3fr)_auto] items-center gap-2">
      <span className="truncate font-sans text-xs text-tx-2" title={item.name}>
        {item.name}
      </span>
      <MeterTrack
        item={item}
        displayMode={displayMode}
        showThresholdMarkers={showThresholdMarkers}
        orientation="horizontal"
      />
      <span className="max-w-24 truncate font-mono text-xs tabular-nums text-tx-1">
        {item.display.text}
      </span>
    </div>
  );
}

export function VerticalBarGauge({
  item,
  displayMode,
  showThresholdMarkers,
}: BarGaugeProps) {
  return (
    <div className="grid h-full min-h-24 min-w-14 grid-rows-[auto_minmax(3rem,1fr)_auto] justify-items-center gap-1">
      <span className="max-w-full truncate font-mono text-xs tabular-nums text-tx-1">
        {item.display.text}
      </span>
      <MeterTrack
        item={item}
        displayMode={displayMode}
        showThresholdMarkers={showThresholdMarkers}
        orientation="vertical"
      />
      <span
        className="max-w-full truncate font-sans text-type-micro text-tx-3"
        title={item.name}
      >
        {item.name}
      </span>
    </div>
  );
}

interface BarGaugeProps {
  item: BarGaugeValue;
  displayMode: 'basic' | 'thresholds';
  showThresholdMarkers: boolean;
}

function MeterTrack({
  item,
  displayMode,
  showThresholdMarkers,
  orientation,
}: BarGaugeProps & { orientation: 'horizontal' | 'vertical' }) {
  const vertical = orientation === 'vertical';
  return (
    <div
      role="meter"
      aria-label={item.name}
      aria-valuemin={item.range.min}
      aria-valuemax={item.range.max}
      aria-valuenow={item.value}
      aria-valuetext={item.display.text}
      title={`${item.name}: ${item.display.text} (${item.minimumText}–${item.maximumText})`}
      className={cn(
        'relative isolate overflow-hidden rounded-sm bg-bg-3',
        vertical ? 'h-full w-5' : 'h-3 w-full',
      )}
    >
      {displayMode === 'thresholds' &&
        item.intervals.map((interval) => {
          const start = valueRatio(interval.start, item.range) * 100;
          const end = valueRatio(interval.end, item.range) * 100;
          return (
            <span
              aria-hidden="true"
              key={`${interval.start}:${interval.end}:${interval.color}`}
              className="absolute"
              style={
                vertical
                  ? {
                      bottom: `${start}%`,
                      height: `${end - start}%`,
                      insetInline: 0,
                      backgroundColor: interval.color,
                      opacity: 0.22,
                    }
                  : {
                      left: `${start}%`,
                      width: `${end - start}%`,
                      insetBlock: 0,
                      backgroundColor: interval.color,
                      opacity: 0.22,
                    }
              }
            />
          );
        })}
      <span
        data-testid="bar-gauge-fill"
        aria-hidden="true"
        className="absolute"
        style={
          vertical
            ? {
                insetInline: 0,
                bottom: 0,
                height: `${item.ratio * 100}%`,
                backgroundColor: item.color,
              }
            : {
                insetBlock: 0,
                left: 0,
                width: `${item.ratio * 100}%`,
                backgroundColor: item.color,
              }
        }
      />
      {showThresholdMarkers &&
        item.markers.map((marker) => {
          const position = valueRatio(marker, item.range) * 100;
          return (
            <span
              aria-hidden="true"
              data-testid="bar-gauge-threshold-marker"
              key={marker}
              className={cn(
                'absolute z-10 bg-bg-0 opacity-80',
                vertical ? 'h-px w-full' : 'h-full w-px',
              )}
              style={vertical ? { bottom: `${position}%` } : { left: `${position}%` }}
            />
          );
        })}
    </div>
  );
}

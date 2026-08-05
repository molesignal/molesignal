/*
 * Responsive layout adapted from Grafana UI v13.1.0 BigValue and Sparkline.
 * Copyright 2015 Grafana Labs. Licensed under Apache License 2.0.
 * Modified for MoleSignal's local tokens, data model, and accessibility rules.
 */

import { cn } from '@/shell/lib/cn';

import { formatPercentChange, type StatValue } from './model';
import { Sparkline } from './Sparkline';
import { useElementSize } from '../shared/MeasuredContainer';

type TextMode = 'value' | 'value_and_name' | 'name';
type GraphMode = 'none' | 'area';
type ColorMode = 'none' | 'value' | 'background';

export function BigValue({
  item,
  height,
  textMode,
  graphMode,
  colorMode,
  showPercentChange,
}: {
  item: StatValue;
  height: number;
  textMode: TextMode;
  graphMode: GraphMode;
  colorMode: ColorMode;
  showPercentChange: boolean;
}) {
  const [ref, size] = useElementSize({ width: 240, height });
  const hasGraph = graphMode === 'area' && item.sparkline.length > 1;
  const wide = hasGraph && size.width / Math.max(1, size.height) > 2.5;
  const compact = size.height < 100 || size.width < 150;
  const mainText = textMode === 'name' ? item.name : item.display.text;
  const showName = textMode === 'value_and_name';
  const valueColor = colorMode === 'value' ? item.color : undefined;
  const backgroundStyle =
    colorMode === 'background'
      ? { backgroundColor: `color-mix(in srgb, ${item.color} 14%, transparent)` }
      : undefined;
  const fontSize = bigValueFontSize(
    mainText,
    wide ? size.width * 0.5 : size.width,
    compact ? size.height * 0.5 : size.height * 0.58,
  );
  const change =
    showPercentChange && item.percentChange !== null
      ? formatPercentChange(item.percentChange)
      : null;

  return (
    <div
      ref={ref}
      role="img"
      aria-label={`${item.name}: ${item.display.text}${change ? `; ${change}` : ''}`}
      className={cn(
        'relative isolate flex h-full min-h-20 min-w-0 overflow-hidden rounded-sm px-3 py-2',
        wide ? 'items-center gap-4' : 'flex-col items-center justify-center text-center',
      )}
      style={backgroundStyle}
    >
      <div
        className={cn(
          'relative z-10 min-w-0',
          wide ? 'flex w-1/2 flex-col items-start' : 'w-full',
        )}
      >
        <div
          className="truncate font-mono font-semibold leading-none tracking-tight text-tx-0"
          style={{ fontSize, ...(valueColor ? { color: valueColor } : {}) }}
          title={mainText}
        >
          {mainText}
        </div>
        {(showName || change) && (
          <div
            className={cn(
              'mt-2 flex min-w-0 items-center gap-2 font-sans text-xs',
              wide ? 'justify-start' : 'justify-center',
            )}
          >
            {showName && <span className="truncate text-tx-3">{item.name}</span>}
            {change && (
              <span
                className={cn(
                  'shrink-0 font-mono tabular-nums',
                  (item.percentChange ?? 0) > 0
                    ? 'text-green'
                    : (item.percentChange ?? 0) < 0
                      ? 'text-red'
                      : 'text-tx-3',
                )}
              >
                {change}
              </span>
            )}
          </div>
        )}
      </div>
      {hasGraph && (
        <Sparkline
          values={item.sparkline}
          color={item.color}
          className={cn(
            wide
              ? 'h-[70%] min-h-10 w-1/2'
              : 'pointer-events-none absolute inset-x-0 bottom-0 h-[42%] w-full',
          )}
        />
      )}
    </div>
  );
}

export function bigValueFontSize(
  text: string,
  availableWidth: number,
  availableHeight: number,
): number {
  const widthSize = availableWidth / Math.max(2, text.length * 0.58);
  return Math.round(Math.max(18, Math.min(64, availableHeight, widthSize)));
}

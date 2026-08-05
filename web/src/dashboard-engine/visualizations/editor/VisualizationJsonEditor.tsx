import { Switch } from '@/shell/ui/switch';
import type { TimeSeriesLegendStat } from '@/viz/timeseries/types';

import {
  LegendModeControl,
  type LegendMode,
} from './legend/LegendModeControl';
import { LegendStatsControl } from './legend/LegendStatsControl';
import { useDashboardText } from '../../i18n';
import type { VisualizationEditorProps } from '../shared/types';

const CONTROL_CLASS =
  'min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 text-xs text-tx-1 outline-none focus-visible:bg-bg-2 focus-visible:text-tx-0';

export function VisualizationJsonEditor({
  options,
  onChange,
}: VisualizationEditorProps) {
  const tr = useDashboardText();
  const setOption = (key: string, value: unknown) =>
    onChange({ ...options, [key]: value });
  return (
    <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0 bg-bg-0">
      {Object.entries(options).map(([key, value]) => {
        const label = tr(optionLabel(key));
        const choices = OPTION_CHOICES[key];
        if (key === 'legendMode' && isLegendMode(value)) {
          return (
            <div
              key={key}
              className="grid min-h-11 grid-cols-[minmax(7rem,1fr)_minmax(0,1.4fr)] items-center gap-3 px-3 py-2 font-sans text-xs"
            >
              <span className="text-tx-2">{label}</span>
              <LegendModeControl
                value={value}
                label={label}
                optionLabel={(mode) => tr(optionLabel(mode))}
                onChange={(mode) => setOption(key, mode)}
              />
            </div>
          );
        }
        if (key === 'legendStats' && Array.isArray(value)) {
          const stats = value.filter(isTimeSeriesLegendStat);
          const legendValuesLabel = tr('Legend values');
          return (
            <div
              key={key}
              className="grid min-h-11 grid-cols-[minmax(7rem,1fr)_minmax(0,1.4fr)] items-center gap-3 px-3 py-2 font-sans text-xs"
            >
              <span className="text-tx-2">{legendValuesLabel}</span>
              <LegendStatsControl
                value={stats}
                label={legendValuesLabel}
                placeholder={tr('Select calculations')}
                searchPlaceholder={tr('Search calculations')}
                emptyLabel={tr('No calculations found')}
                removeText={tr('Remove')}
                optionLabel={(stat) =>
                  tr(stat === 'sum' ? 'Total' : optionLabel(stat))
                }
                onChange={(nextStats) => setOption(key, nextStats)}
              />
            </div>
          );
        }
        return (
          <label
            key={key}
            className="grid min-h-11 grid-cols-[minmax(7rem,1fr)_minmax(0,1.4fr)] items-center gap-3 px-3 py-2 font-sans text-xs"
          >
            <span className="text-tx-2">{label}</span>
            {typeof value === 'boolean' ? (
              <span className="flex justify-end">
                <Switch
                  aria-label={label}
                  checked={value}
                  onCheckedChange={(checked) => setOption(key, checked)}
                />
              </span>
            ) : typeof value === 'number' ? (
              <input
                type="number"
                value={value}
                step={key.toLowerCase().includes('opacity') ? 0.1 : 'any'}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  if (Number.isFinite(next)) setOption(key, next);
                }}
                className={`${CONTROL_CLASS} h-8 font-mono`}
              />
            ) : typeof value === 'string' && choices ? (
              <select
                value={value}
                onChange={(event) => setOption(key, event.target.value)}
                className={`${CONTROL_CLASS} h-8 font-sans`}
              >
                {choices.map((choice) => (
                  <option key={choice} value={choice}>
                    {tr(optionLabel(choice))}
                  </option>
                ))}
              </select>
            ) : typeof value === 'string' ? (
              key === 'content' ? (
                <textarea
                  value={value}
                  rows={4}
                  onChange={(event) => setOption(key, event.target.value)}
                  className={`${CONTROL_CLASS} resize-y py-1.5 font-sans leading-5`}
                />
              ) : (
                <input
                  value={value}
                  onChange={(event) => setOption(key, event.target.value)}
                  className={`${CONTROL_CLASS} h-8 font-sans`}
                />
              )
            ) : Array.isArray(value) ? (
              <input
                value={value.map(String).join(', ')}
                placeholder={tr('Comma-separated values')}
                onChange={(event) =>
                  setOption(
                    key,
                    event.target.value
                      .split(',')
                      .map((item) => item.trim())
                      .filter(Boolean),
                  )
                }
                className={`${CONTROL_CLASS} h-8 font-sans`}
              />
            ) : (
              <span className="rounded-md border border-dashed border-bd-1 px-2 py-2 font-sans text-xs leading-5 text-tx-3">
                {tr('Imported structured option is preserved')}
              </span>
            )}
          </label>
        );
      })}
    </div>
  );
}

function isLegendMode(value: unknown): value is LegendMode {
  return value === 'list' || value === 'table' || value === 'hidden';
}

function isTimeSeriesLegendStat(
  value: unknown,
): value is TimeSeriesLegendStat {
  return (
    value === 'last' ||
    value === 'min' ||
    value === 'max' ||
    value === 'mean' ||
    value === 'sum'
  );
}

export function optionLabel(value: string): string {
  return value
    .replace(/[_-]+/g, ' ')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/^./, (character) => character.toUpperCase());
}

export const OPTION_CHOICES: Record<string, readonly string[]> = {
  drawStyle: ['line', 'area', 'bar', 'points'],
  lineInterpolation: ['linear', 'stepBefore', 'stepAfter'],
  showPoints: ['auto', 'always', 'never'],
  stackMode: ['none', 'normal', 'percent'],
  tooltipMode: ['single', 'all', 'hidden'],
  legendMode: ['list', 'table', 'hidden'],
  legendPlacement: ['bottom', 'right'],
  sortOrder: ['desc', 'asc'],
  deduplication: ['none', 'exact', 'numbers', 'signature'],
  calculation: ['last', 'min', 'max', 'mean', 'avg', 'sum'],
  textMode: ['value', 'value_and_name', 'name'],
  graphMode: ['none', 'area'],
  colorMode: ['none', 'value', 'background'],
  displayMode: ['basic', 'thresholds'],
  orientation: ['horizontal', 'vertical'],
  colorScheme: ['blues', 'greens', 'reds', 'spectrum'],
  showValues: ['auto', 'always', 'never'],
  mode: ['markdown', 'plain'],
};

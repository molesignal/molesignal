import { MousePointer2, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { TimezoneSelect } from '@/shell/TimezoneSelect';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import type { MetricsDrawStyle, MetricsStackMode } from '../model';
import type { MetricsExploreResultsProps } from './types';

const DRAW_STYLES = [
  ['line', 'explore.chart.mode_line'],
  ['bar', 'explore.chart.mode_bar'],
  ['points', 'explore.chart.mode_points'],
] as const;

/**
 * How the graph is drawn: its zoom state, mark, stacking and the timezone of
 * its time axis. They sit in the results header beside the view tabs, so the
 * chart keeps the row a second toolbar would take.
 */
export function ChartControls({
  chart,
  canStack,
}: {
  chart: MetricsExploreResultsProps['chart'];
  canStack: boolean;
}) {
  const { t } = useTranslation('metrics');

  return (
    <>
      {chart.zoomed ? (
        <ChromeButton
          size="sm"
          onClick={chart.onRangeReset}
          title={t('explore.chart.reset_zoom_hint')}
          data-testid="metrics-reset-zoom"
          className="h-11 sm:h-8"
        >
          <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
          {t('explore.chart.reset_zoom')}
        </ChromeButton>
      ) : (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label={t('explore.chart.zoom_hint')}
              className="hidden h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1 sm:grid"
            >
              <MousePointer2 className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{t('explore.chart.zoom_hint')}</TooltipContent>
        </Tooltip>
      )}
      <DrawStyleControl value={chart.drawStyle} onChange={chart.onDrawStyleChange} />
      <StackModeControl
        value={canStack ? chart.stackMode : 'none'}
        disabled={!canStack}
        onChange={chart.onStackModeChange}
      />
      <TimezoneSelect
        value={chart.timezone}
        onChange={chart.onTimezoneChange}
        className="h-11 max-w-[220px] sm:h-8"
      />
    </>
  );
}

function DrawStyleControl({
  value,
  onChange,
}: {
  value: MetricsDrawStyle;
  onChange: (style: MetricsDrawStyle) => void;
}) {
  const { t } = useTranslation('metrics');

  return (
    <div
      role="group"
      aria-label={t('explore.chart.draw_style')}
      className="flex h-11 shrink-0 items-center rounded-md bg-[var(--control-surface)] p-0.5 sm:h-8"
    >
      {DRAW_STYLES.map(([id, labelKey]) => (
        <button
          key={id}
          type="button"
          onClick={() => onChange(id)}
          aria-pressed={value === id}
          className={`h-full rounded px-2 font-sans text-xs transition-colors focus-visible:bg-bg-3 ${
            value === id
              ? 'bg-[var(--functional-surface)] font-semibold text-tx-0'
              : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0'
          }`}
        >
          {t(labelKey)}
        </button>
      ))}
    </div>
  );
}

function StackModeControl({
  value,
  disabled,
  onChange,
}: {
  value: MetricsStackMode;
  disabled: boolean;
  onChange: (mode: MetricsStackMode) => void;
}) {
  const { t } = useTranslation('metrics');

  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(next) => onChange(next as MetricsStackMode)}
    >
      <SelectTrigger
        aria-label={t('explore.chart.stack_mode')}
        title={disabled ? t('explore.chart.stack_requires_multiple_series') : undefined}
        className="h-11 w-[8.75rem] shrink-0 px-2 text-base sm:h-8 sm:text-xs"
        data-testid="metrics-stack-mode"
      >
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="shrink-0 text-tx-2">{t('explore.chart.stack_mode')}</span>
          <SelectValue />
        </span>
      </SelectTrigger>
      <SelectContent align="end">
        <SelectItem value="none" className="text-xs">
          {t('explore.chart.stack_none')}
        </SelectItem>
        <SelectItem value="normal" className="text-xs">
          {t('explore.chart.stack_normal')}
        </SelectItem>
        <SelectItem value="percent" className="text-xs">
          {t('explore.chart.stack_percent')}
        </SelectItem>
      </SelectContent>
    </Select>
  );
}

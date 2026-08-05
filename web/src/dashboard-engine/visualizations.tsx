import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { dataFrameToObjects } from './dataframe';
import { formatFieldValue } from './fieldConfig';
import type {
  DashboardPanel,
  DataField,
  DataFrame,
  PanelData,
  VisualizationType,
} from './schema';
import { BarChartVisualization } from './visualizations/bar-chart';
import { BarGaugeVisualization } from './visualizations/bar-gauge';
import { VisualizationJsonEditor } from './visualizations/editor/VisualizationJsonEditor';
import { GaugeVisualization } from './visualizations/gauge';
import { HeatmapVisualization } from './visualizations/heatmap';
import { resolveVisualizationOptions } from './visualizations/options';
import { EmptyVisualization } from './visualizations/shared/EmptyVisualization';
import type {
  VisualizationEditorProps,
  VisualizationProps,
} from './visualizations/shared/types';
import {
  visualizationStatusKind,
  VisualizationStatus,
} from './visualizations/shared/VisualizationStatus';
import { StatVisualization } from './visualizations/stat';
import { StateTimelineVisualization } from './visualizations/state-timeline';
import { TimeSeriesVisualization } from './visualizations/time-series';

export type {
  VisualizationEditorProps,
  VisualizationProps,
} from './visualizations/shared/types';

export interface VisualizationPlugin<
  TOptions = Record<string, unknown>,
> {
  id: VisualizationType;
  name: string;
  description: string;
  component: React.ComponentType<VisualizationProps<TOptions>>;
  editor: React.ComponentType<VisualizationEditorProps<TOptions>>;
  defaultOptions: TOptions;
  supports: (data: PanelData) => boolean;
  recommendationScore: (data: PanelData) => number;
  optionSchemaVersion: number;
}

class VisualizationRegistry {
  private readonly plugins = new Map<
    VisualizationType,
    VisualizationPlugin<Record<string, unknown>>
  >();

  register<TOptions extends Record<string, unknown>>(
    plugin: VisualizationPlugin<TOptions>,
  ): void {
    this.plugins.set(
      plugin.id,
      plugin as VisualizationPlugin<Record<string, unknown>>,
    );
  }

  get(type: VisualizationType): VisualizationPlugin<Record<string, unknown>> {
    return this.plugins.get(type) ?? this.plugins.get('table')!;
  }

  list(): Array<VisualizationPlugin<Record<string, unknown>>> {
    return [...this.plugins.values()];
  }

  recommend(data: PanelData): VisualizationType {
    return this.list()
      .filter((plugin) => plugin.supports(data))
      .sort(
        (left, right) =>
          right.recommendationScore(data) - left.recommendationScore(data),
      )[0]?.id ?? 'table';
  }
}

export const visualizationRegistry = new VisualizationRegistry();

const plugins: Array<VisualizationPlugin<Record<string, unknown>>> = [
  plugin('time_series', 'Time series', 'Values over time', TimeSeriesVisualization, {
    drawStyle: 'line',
    lineInterpolation: 'linear',
    lineWidth: 1.5,
    fillOpacity: 0,
    showPoints: 'auto',
    stackMode: 'none',
    tooltipMode: 'all',
    legendMode: 'table',
    legendPlacement: 'bottom',
    legendStats: ['last', 'min', 'max', 'mean'],
  }, hasTimeAndNumber, (data) => (hasTimeAndNumber(data) ? 100 : 0)),
  plugin('table', 'Table', 'Rows and fields', TableVisualization, {
    showHeader: true,
    striped: false,
    pageSize: 100,
  }, hasFields, (data) => (hasFields(data) ? 45 : 0)),
  plugin('logs', 'Logs', 'Timestamped event lines', LogsVisualization, {
    showTime: true,
    showLevel: true,
    wrapLines: false,
    prettifyJson: true,
    sortOrder: 'desc',
    deduplication: 'none',
  }, hasStringField, (data) => (sourceType(data, 'logs') ? 100 : 25)),
  plugin('stat', 'Stat', 'Single reduced value', StatVisualization, {
    calculation: 'last',
    textMode: 'value_and_name',
    graphMode: 'none',
    colorMode: 'value',
    showPercentChange: false,
  }, hasNumber, (data) => (hasNumber(data) ? 55 : 0)),
  plugin('gauge', 'Gauge', 'Value against a range', GaugeVisualization, {
    calculation: 'last',
    showThresholdMarkers: true,
    showThresholdLabels: false,
  }, hasNumber, (data) => (hasNumber(data) ? 40 : 0)),
  plugin('bar_gauge', 'Bar gauge', 'Comparable values as bars', BarGaugeVisualization, {
    orientation: 'horizontal',
    calculation: 'last',
    displayMode: 'basic',
    showThresholdMarkers: true,
  }, hasNumber, (data) => (hasNumber(data) ? 50 : 0)),
  plugin('bar_chart', 'Bar chart', 'Categorical value comparison', BarChartVisualization, {
    orientation: 'vertical',
    groupWidth: 0.7,
    calculation: 'last',
    showValues: 'auto',
  }, hasNumber, (data) => (hasNumber(data) ? 48 : 0)),
  plugin('heatmap', 'Heatmap', 'Value density over time', HeatmapVisualization, {
    colorScheme: 'blues',
  }, hasNumber, (data) => (hasTimeAndNumber(data) ? 35 : 0)),
  plugin(
    'state_timeline',
    'State timeline',
    'Discrete states over time',
    StateTimelineVisualization,
    { mergeEqual: true, showValues: 'auto' },
    hasTimeField,
    (data) => (hasTimeField(data) ? 35 : 0),
  ),
  plugin('text', 'Text', 'Markdown or plain text', TextVisualization, {
    mode: 'markdown',
    content: '',
  }, () => true, () => 5),
];

for (const item of plugins) visualizationRegistry.register(item);

export function VisualizationRenderer({
  panel,
  data,
  height,
  cursorScopeId,
}: {
  panel: DashboardPanel;
  data: PanelData;
  height: number;
  cursorScopeId?: string | null | undefined;
}) {
  const status = visualizationStatusKind(data);
  if (status) {
    return (
      <VisualizationStatus
        kind={status}
        detail={status === 'error' ? data.error?.message : undefined}
      />
    );
  }
  const plugin = visualizationRegistry.get(panel.visualization.type);
  const Component = plugin.component;
  const options = resolveVisualizationOptions(
    plugin.defaultOptions,
    panel.visualization.options,
  );
  return (
    <Component
      panel={panel}
      data={data}
      options={options}
      height={height}
      cursorScopeId={cursorScopeId}
    />
  );
}

function plugin(
  id: VisualizationType,
  name: string,
  description: string,
  component: React.ComponentType<VisualizationProps>,
  defaultOptions: Record<string, unknown>,
  supports: (data: PanelData) => boolean,
  recommendationScore: (data: PanelData) => number,
): VisualizationPlugin<Record<string, unknown>> {
  return {
    id,
    name,
    description,
    component,
    editor: VisualizationJsonEditor,
    defaultOptions,
    supports,
    recommendationScore,
    optionSchemaVersion: 1,
  };
}

function TableVisualization({
  data,
  options,
}: VisualizationProps) {
  const frame = data.frames[0];
  if (!frame || frame.length === 0) return <EmptyVisualization />;
  const pageSize = Math.max(1, optionNumber(options.pageSize, 100));
  const rows = dataFrameToObjects(frame).slice(0, pageSize);
  return (
    <div className="h-full overflow-auto">
      <table className="w-full border-separate border-spacing-0 font-sans text-xs">
        {options.showHeader !== false && (
          <thead className="sticky top-0 z-10 bg-bg-2 text-left text-tx-2">
            <tr>
              {frame.fields.map((field) => (
                <th
                  key={field.id}
                  className="border-b border-bd-1 px-2 py-1.5 font-medium"
                >
                  {field.config?.displayName ?? field.name}
                </th>
              ))}
            </tr>
          </thead>
        )}
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr
              key={rowIndex}
              className={cn(
                'hover:bg-bg-2',
                options.striped === true && rowIndex % 2 === 1 && 'bg-bg-2/50',
              )}
            >
              {frame.fields.map((field) => {
                const display = formatFieldValue(
                  row[field.name],
                  field.config,
                );
                return (
                  <td
                    key={field.id}
                    className="max-w-[36rem] border-b border-bd-0 px-2 py-1.5 align-top text-tx-1"
                    style={display.color ? { color: display.color } : undefined}
                  >
                    <span className="break-words">{display.text}</span>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function LogsVisualization({
  data,
  options,
}: VisualizationProps) {
  const frame = data.frames[0];
  if (!frame || frame.length === 0) return <EmptyVisualization />;
  const fields = chooseLogFields(frame, options);
  let rows = dataFrameToObjects(frame);
  if (options.sortOrder === 'desc') rows = [...rows].reverse();
  return (
    <div className="h-full overflow-auto font-mono text-xs">
      {rows.map((row, index) => (
        <div
          key={index}
          className="grid grid-cols-[auto_minmax(0,1fr)] gap-2 border-b border-bd-0 px-2 py-1.5 hover:bg-bg-2"
        >
          <span className="select-none text-tx-3">{index + 1}</span>
          <div
            className={cn(
              'min-w-0 text-tx-1',
              options.wrapLines !== true && 'truncate whitespace-nowrap',
            )}
          >
            {fields.map((field) => (
              <span key={field.id} className="mr-3">
                <span className="text-tx-3">{field.name}=</span>
                {formatLogValue(row[field.name], options.prettifyJson === true)}
              </span>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function TextVisualization({ options }: VisualizationProps) {
  return (
    <div
      className={cn(
        'h-full overflow-auto whitespace-pre-wrap font-sans text-sm leading-6 text-tx-1',
        options.mode === 'plain' && 'font-mono text-xs',
      )}
    >
      {stringOption(options.content, '')}
    </div>
  );
}

function chooseLogFields(
  frame: DataFrame,
  options: Record<string, unknown>,
): DataField[] {
  const configured = Array.isArray(options.fields)
    ? options.fields.filter(
        (value): value is string => typeof value === 'string',
      )
    : [];
  let fields =
    configured.length > 0
      ? configured
          .map((name) => frame.fields.find((field) => field.name === name))
          .filter((field): field is DataField => Boolean(field))
      : frame.fields;
  if (options.showTime === false) {
    fields = fields.filter((field) => field.type !== 'time');
  }
  if (options.showLevel === false) {
    fields = fields.filter((field) => field.name.toLowerCase() !== 'level');
  }
  return fields;
}

function formatLogValue(value: unknown, prettify: boolean): string {
  if (prettify && typeof value === 'string') {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return typeof value === 'object' ? JSON.stringify(value) : String(value ?? '');
}

function hasFields(data: PanelData): boolean {
  return data.frames.some((frame) => frame.fields.length > 0);
}

function hasNumber(data: PanelData): boolean {
  return data.frames.some((frame) =>
    frame.fields.some((field) => field.type === 'number'),
  );
}

function hasTimeField(data: PanelData): boolean {
  return data.frames.some((frame) =>
    frame.fields.some((field) => field.type === 'time'),
  );
}

function hasStringField(data: PanelData): boolean {
  return data.frames.some((frame) =>
    frame.fields.some(
      (field) => field.type === 'string' || field.type === 'json',
    ),
  );
}

function hasTimeAndNumber(data: PanelData): boolean {
  return hasTimeField(data) && hasNumber(data);
}

function sourceType(data: PanelData, type: string): boolean {
  return data.frames.some((frame) => frame.meta?.sourceType === type);
}

function stringOption(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value : fallback;
}

function optionNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

import { ChartColumn, ChartLine, Hash, Table2, type LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { QueryToolbarButton, QueryToolbarGroup } from '@/shell/query/Workbench';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shell/ui/select';

import type { LogFieldDef } from '../fieldQueryModel';
import {
  groupableFields,
  isNumericMeasure,
  measurableFields,
  STATS_GROUP_NONE,
  STATS_GROUP_TIME,
  STATS_LIMITS,
  STATS_MEASURES,
  type LogStatsConfig,
  type StatsChart,
  type StatsMeasure,
} from './model';

const TRIGGER =
  'h-8 w-auto min-w-[104px] max-w-[220px] gap-2 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-0 shadow-none sm:h-8';
/** Radix refuses an item with an empty value: no group has a name of its own inside the select. */
const NO_GROUP = '@none';

interface PickerProps {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
  testId: string;
}

function Picker({ label, value, options, onChange, testId }: PickerProps) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={label} data-log-stats-control={testId} className={TRIGGER}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value} className="font-sans text-xs">
            <span className="truncate">{option.label}</span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

const CHART_ICON: Record<StatsChart, LucideIcon> = {
  stat: Hash,
  bar: ChartColumn,
  line: ChartLine,
  table: Table2,
};

interface ChartSwitchProps {
  charts: readonly StatsChart[];
  value: StatsChart;
  onChange: (chart: StatsChart) => void;
}

/** How the result is drawn: the ways it can be, the one it is in lit. */
export function LogStatsChartSwitch({ charts, value, onChange }: ChartSwitchProps) {
  const { t } = useTranslation('logs');

  return (
    <QueryToolbarGroup
      role="group"
      aria-label={t('explore.stats.chart_aria')}
      className="h-8 border-0 bg-[var(--control-surface)]"
    >
      {charts.map((chart) => {
        const Icon = CHART_ICON[chart];
        const label = t(`explore.stats.charts.${chart}`);

        return (
          <QueryToolbarButton
            key={chart}
            active={chart === value}
            flat
            onClick={() => onChange(chart)}
            className="h-7 w-8 px-0"
            aria-label={label}
            aria-pressed={chart === value}
            title={label}
            data-log-stats-chart={chart}
          >
            <Icon aria-hidden="true" className="h-3.5 w-3.5" />
          </QueryToolbarButton>
        );
      })}
    </QueryToolbarGroup>
  );
}

interface LogStatsControlsProps {
  fields: readonly LogFieldDef[];
  config: LogStatsConfig;
  onChange: (config: LogStatsConfig) => void;
}

/**
 * What the statistics view works out, as one sentence of controls: a measure,
 * the field it is of, what the logs are grouped by, and how many groups are
 * kept. Every choice is one the stream's fields can answer.
 */
export function LogStatsControls({ fields, config, onChange }: LogStatsControlsProps) {
  const { t } = useTranslation('logs');
  const measured = measurableFields(fields, config.measure);
  const grouped = config.groupBy !== STATS_GROUP_NONE && config.groupBy !== STATS_GROUP_TIME;
  // A measure of numbers is offered only where the stream has a number to take it of.
  const hasNumbers = fields.some((field) => field.dataType === 'int64' || field.dataType === 'float64');
  const measures = STATS_MEASURES.filter((measure) => hasNumbers || !isNumericMeasure(measure));

  return (
    <div data-log-stats-controls className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1.5 font-sans text-xs text-tx-2">
      <span>{t('explore.stats.measure')}</span>
      <Picker
        testId="measure"
        label={t('explore.stats.measure_aria')}
        value={config.measure}
        options={measures.map((measure) => ({ value: measure, label: t(`explore.stats.measures.${measure}`) }))}
        onChange={(measure) => onChange({ ...config, measure: measure as StatsMeasure })}
      />
      {config.measure !== 'count' && (
        <Picker
          testId="field"
          label={t('explore.stats.field_aria')}
          value={config.field}
          options={measured.map((field) => ({ value: field.name, label: field.name }))}
          onChange={(field) => onChange({ ...config, field })}
        />
      )}
      <span>{t('explore.stats.group')}</span>
      <Picker
        testId="group"
        label={t('explore.stats.group_aria')}
        value={config.groupBy === STATS_GROUP_NONE ? NO_GROUP : config.groupBy}
        options={[
          { value: STATS_GROUP_TIME, label: t('explore.stats.group_time') },
          { value: NO_GROUP, label: t('explore.stats.group_none') },
          ...groupableFields(fields).map((field) => ({ value: field.name, label: field.name })),
        ]}
        onChange={(groupBy) => onChange({ ...config, groupBy: groupBy === NO_GROUP ? STATS_GROUP_NONE : groupBy })}
      />
      {grouped && (
        <>
          <span>{t('explore.stats.limit')}</span>
          <Picker
            testId="limit"
            label={t('explore.stats.limit_aria')}
            value={String(config.limit)}
            options={STATS_LIMITS.map((limit) => ({ value: String(limit), label: String(limit) }))}
            onChange={(limit) => onChange({ ...config, limit: Number(limit) })}
          />
        </>
      )}
    </div>
  );
}

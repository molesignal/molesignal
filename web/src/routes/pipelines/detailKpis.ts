import type { TFunction } from 'i18next';

import type { TodaysAlertRecord } from './alertRecord';
import type { summarizePipelineRuns } from './presentation';
import { formatMillisDuration, formatRelativeMicros } from './presentation';
import type { PipelineKpiItem } from './Surfaces';

type RunSummary = Pick<
  ReturnType<typeof summarizePipelineRuns>,
  'lastRun' | 'runs24h' | 'successRate' | 'processedRows' | 'averageDuration' | 'completedRuns'
>;

/**
 * The figures at the top of a pipeline's overview: for an alert pipeline what
 * it did with today's alert events, for any other its runs of the last day.
 */
export function pipelineKpis(
  alertRecord: TodaysAlertRecord | null,
  runs: RunSummary,
  t: TFunction<'pipelines'>,
  language: string,
): PipelineKpiItem[] {
  if (alertRecord) {
    return [
      { label: t('alert_mode.kpi.processed'), value: String(alertRecord.processed), note: t('alert_mode.kpi.today') },
      {
        label: t('alert_mode.kpi.dropped'),
        value: String(alertRecord.dropped),
        note: alertRecord.lastDropped
          ? t('alert_mode.kpi.last', {
              rule: alertRecord.lastDropped.rule,
              when: formatRelativeMicros(alertRecord.lastDropped.atTime, language),
            })
          : t('alert_mode.kpi.today'),
      },
      {
        label: t('alert_mode.kpi.failed'),
        value: String(alertRecord.failed),
        note: alertRecord.lastFailed?.reason ?? t('alert_mode.kpi.today'),
        tone: alertRecord.failed > 0 ? 'neutral' : 'good',
      },
    ];
  }

  const { lastRun, runs24h, successRate, processedRows, averageDuration, completedRuns } = runs;

  return [
    {
      label: t('detail.overview.last_run'),
      value: formatRelativeMicros(lastRun?.startedAt, language),
      note: lastRun ? t(`overview.run_states.${lastRun.state}`, { defaultValue: lastRun.state }) : '—',
    },
    {
      label: t('detail.overview.success_rate'),
      value: successRate == null ? '—' : `${successRate.toFixed(successRate === 100 ? 0 : 1)}%`,
      note: t('detail.overview.window_24h', { count: runs24h.length }),
      tone: successRate != null && successRate >= 99 ? 'good' : 'neutral',
    },
    {
      label: t('detail.overview.processed_rows'),
      value: new Intl.NumberFormat(language, { notation: 'compact' }).format(processedRows),
      note: t('detail.overview.window_24h', { count: runs24h.length }),
    },
    {
      label: t('detail.overview.average_duration'),
      value: averageDuration == null ? '—' : formatMillisDuration(averageDuration),
      note: t('detail.overview.completed_runs', { count: completedRuns }),
    },
  ];
}

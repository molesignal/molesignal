import { Activity, FileCode2, Gauge } from 'lucide-react';
import type * as React from 'react';

import type { ComparisonOp, Severity, StreamType } from '@/types/alerting';

/** The signals a rule over a query is made on. */
export type RuleSignal = StreamType;

export interface QueryPreset {
  id: string;
  titleKey: string;
  descriptionKey: string;
  stream: string;
  statement: string;
  operator: ComparisonOp;
  threshold: number;
  severity: Severity;
}

/** The queries a rule starts from, by signal; the first of each is what choosing the signal fills in. */
export const PRESETS: Record<RuleSignal, QueryPreset[]> = {
  metrics: [
    {
      id: 'error-rate',
      titleKey: 'workbench.presets.error_rate.title',
      descriptionKey: 'workbench.presets.error_rate.description',
      stream: 'http_requests_total',
      statement:
        'sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m]))',
      operator: 'gt',
      threshold: 0.05,
      severity: 'warning',
    },
    {
      id: 'latency',
      titleKey: 'workbench.presets.latency.title',
      descriptionKey: 'workbench.presets.latency.description',
      stream: 'http_request_duration_seconds',
      statement:
        'histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))',
      operator: 'gt',
      threshold: 0.8,
      severity: 'warning',
    },
    {
      id: 'cpu',
      titleKey: 'workbench.presets.cpu.title',
      descriptionKey: 'workbench.presets.cpu.description',
      stream: 'process_cpu_usage',
      statement: 'avg(process_cpu_usage)',
      operator: 'gt',
      threshold: 0.85,
      severity: 'warning',
    },
  ],
  logs: [
    {
      id: 'log-errors',
      titleKey: 'workbench.presets.log_errors.title',
      descriptionKey: 'workbench.presets.log_errors.description',
      stream: 'app_logs',
      statement: "SELECT COUNT(*) AS value FROM app_logs WHERE level = 'error'",
      operator: 'gt',
      threshold: 20,
      severity: 'warning',
    },
    {
      id: 'log-no-data',
      titleKey: 'workbench.presets.no_data.title',
      descriptionKey: 'workbench.presets.no_data.description',
      stream: 'app_logs',
      statement: 'SELECT COUNT(*) AS value FROM app_logs',
      operator: 'eq',
      threshold: 0,
      severity: 'warning',
    },
  ],
  traces: [
    {
      id: 'trace-errors',
      titleKey: 'workbench.presets.trace_errors.title',
      descriptionKey: 'workbench.presets.trace_errors.description',
      stream: 'traces',
      statement: "SELECT COUNT(*) AS value FROM traces WHERE status_code = 'ERROR'",
      operator: 'gt',
      threshold: 10,
      severity: 'warning',
    },
    {
      id: 'trace-latency',
      titleKey: 'workbench.presets.trace_latency.title',
      descriptionKey: 'workbench.presets.trace_latency.description',
      stream: 'traces',
      statement: 'SELECT AVG(duration_ns) / 1000000 AS value FROM traces',
      operator: 'gt',
      threshold: 800,
      severity: 'warning',
    },
  ],
};

export const SIGNAL_ICON: Record<RuleSignal, React.ReactNode> = {
  metrics: <Gauge className="h-4 w-4" />,
  logs: <FileCode2 className="h-4 w-4" />,
  traces: <Activity className="h-4 w-4" />,
};

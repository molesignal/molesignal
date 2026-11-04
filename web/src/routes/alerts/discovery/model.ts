import type { TFunction } from 'i18next';

import type { Finding } from '@/api/discovery';
import { sourceOf } from '@/api/discovery';
import { formatDuration, formatRate, formatThroughput } from '@/routes/apm/format';

/** Metrics whose values are fractions: shown as percentages. */
const FRACTIONS = /utilization|filesystem_avail|MemAvailable|cpu/;

/** A figure of a finding in its own unit: a rate, a latency, a throughput, a share or a count. */
export function formatFindingValue(metric: string | null | undefined, value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  if (metric === 'error_rate') return formatRate(value);
  if (metric === 'p95') return formatDuration(value);
  if (metric === 'throughput') return formatThroughput(value);
  if (metric && FRACTIONS.test(metric)) return `${Math.round(value * 100)}%`;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

/** An APM finding's service as its pages name it: the namespace only when it is not the default one. */
export function subjectName(finding: Pick<Finding, 'kind' | 'subject' | 'detail'>): string {
  if (finding.kind.startsWith('rum_')) return finding.detail.application ?? finding.subject;
  if (sourceOf(finding.kind) !== 'apm') return finding.subject;
  return finding.subject.startsWith('default/') ? finding.subject.slice('default/'.length) : finding.subject;
}

/** Where the reader looks into a finding: the service in APM, the errors of the application in RUM,
 * the error logs of the stream, or the series in Metrics. */
export function findingHref(finding: Pick<Finding, 'kind' | 'subject' | 'environment' | 'detail'>): string {
  const source = sourceOf(finding.kind);

  if (source === 'rum') {
    return `/rum/errors?application=${encodeURIComponent(finding.subject)}`;
  }

  if (source === 'logs') {
    const params = new URLSearchParams({ stream: finding.detail.stream ?? finding.subject, streamType: 'logs' });
    return `/logs?${params.toString()}`;
  }

  if (source === 'metrics') {
    const params = new URLSearchParams(finding.detail.promql ? { promql: finding.detail.promql } : {});
    return `/metrics?${params.toString()}`;
  }

  const at = finding.subject.indexOf('/');
  const namespace = at > 0 ? finding.subject.slice(0, at) : 'default';
  const name = at > 0 ? finding.subject.slice(at + 1) : finding.subject;
  const params = new URLSearchParams({ namespace, environment: finding.environment ?? '' });
  return `/apm/services/${encodeURIComponent(name)}?${params}`;
}

type Translate = TFunction<'alerts'>;

/** A finding in one line, in the reader's language: what moved or broke, and where. */
export function findingSummary(finding: Finding, t: Translate): string {
  const subject = subjectName(finding);

  if (finding.kind === 'apm_anomaly') {
    return t('discovery.summary.apm_anomaly', {
      metric: t(`discovery.metrics.${finding.metric ?? 'requests'}`, { defaultValue: finding.metric ?? '' }),
      service: subject,
      value: formatFindingValue(finding.metric, finding.value),
      baseline: formatFindingValue(finding.metric, finding.baseline),
    });
  }

  if (finding.kind === 'apm_regression') {
    return t('discovery.summary.apm_regression', {
      service: subject,
      version: finding.detail.version ?? '',
    });
  }

  if (sourceOf(finding.kind) === 'logs') {
    return t(`discovery.summary.${finding.kind}`, {
      service: subject,
      stream: finding.detail.stream ?? '',
      value: formatFindingValue(finding.metric, finding.value),
      baseline: formatFindingValue(finding.metric, finding.baseline),
      template: finding.detail.template ?? '',
    });
  }

  if (sourceOf(finding.kind) === 'metrics') {
    return t(`discovery.summary.${finding.kind}`, {
      subject,
      metric: finding.metric ?? '',
      value: formatFindingValue(finding.metric, finding.value),
      baseline: formatFindingValue(finding.metric, finding.baseline),
    });
  }

  return t(`discovery.summary.${finding.kind}`, {
    application: subject,
    message: finding.title.split(': ').slice(1).join(': ') || finding.title,
  });
}

/** The question a chat about a finding starts with, in the reader's language. */
export function investigatePrompt(finding: Finding, t: Translate): string {
  return t('discovery.investigate_prompt', { summary: findingSummary(finding, t) });
}

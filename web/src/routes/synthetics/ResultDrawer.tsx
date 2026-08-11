import { ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type {
  MonitorAssertion,
  MonitorRevision,
  ProbeLocation,
  SyntheticMonitor,
  SyntheticResult,
} from '@/api/synthetics';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

import { StatePill } from './components';
import { formatDuration, formatTimestamp, resultDurationMicros } from './model';

export function ResultDrawer({
  result,
  monitor,
  revision,
  locations,
  onClose,
}: {
  result: SyntheticResult | undefined;
  monitor: SyntheticMonitor | undefined;
  revision: MonitorRevision | undefined;
  locations: ProbeLocation[];
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const assertionDefinitions = assertionMap(revision);
  const correlationLinks: Array<[string, string]> = [
    [t('actions.view_trace'), '/traces'],
    [t('actions.view_logs'), '/logs'],
    [t('actions.view_metrics'), '/metrics'],
    [t('actions.open_incident'), '/alerts/incidents'],
    [t('actions.ask_agent'), '/agent'],
    [t('overview.loop_status_page'), '/status-pages'],
  ];
  return (
    <FormDrawer
      open={Boolean(result)}
      onOpenChange={(open) => !open && onClose()}
      title={monitor?.name ?? t('results.title')}
      subtitle={result ? `${formatTimestamp(result.started_at, i18n.language)} · ${locations.find((location) => location.id === result.location_id)?.name ?? result.location_id}` : undefined}
      width={780}
    >
      {result && (
        <>
          <div className="mb-6 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Summary label={t('results.columns.outcome')} value={<StatePill state={result.outcome} />} />
            <Summary label={t('results.columns.run_type')} value={t(result.is_test ? 'results.test_run' : 'results.operational_run')} />
            <Summary label={t('results.columns.duration')} value={formatDuration(resultDurationMicros(result))} />
            <Summary label={t('results.columns.attempts')} value={String(result.attempts.length)} />
          </div>

          <FormSection title={t('detail.timing')}>
            {result.attempts.map((attempt) => (
              <div key={attempt.number} className="rounded-md border border-bd-0 bg-bg-2 p-3">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-xs font-strong text-tx-1">{t('detail.attempt', { number: attempt.number })}</span>
                  <StatePill state={attempt.outcome} compact />
                </div>
                <div className="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-xs sm:grid-cols-5">
                  <Timing label={t('detail.timing_dns')} value={attempt.timing.dns_micros} />
                  <Timing label={t('detail.timing_connect')} value={attempt.timing.connect_micros} />
                  <Timing label={t('detail.timing_tls')} value={attempt.timing.tls_micros} />
                  <Timing label={t('detail.timing_first_byte')} value={attempt.timing.first_byte_micros} />
                  <Timing label={t('detail.timing_total')} value={attempt.timing.total_micros} />
                </div>
                {(attempt.error_message || attempt.error_category) && (
                  <div className="mt-3 rounded border border-red/20 bg-red-dim px-3 py-2 text-xs text-red-soft">
                    {attempt.error_category && <span className="mr-2 font-strong">{attempt.error_category}</span>}
                    {attempt.error_message}
                  </div>
                )}
                {attempt.bounded_response_excerpt && attempt.bounded_response_excerpt.length > 0 && (
                  <pre className="mt-3 max-h-56 overflow-auto rounded border border-bd-0 bg-bg-0 p-3 font-code text-xs leading-relaxed text-tx-1">
                    {decodeExcerpt(attempt.bounded_response_excerpt)}
                  </pre>
                )}
              </div>
            ))}
          </FormSection>

          <FormSection title={t('detail.assertion_evidence')}>
            {result.assertions.length === 0 ? (
              <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center text-xs text-tx-3">{t('detail.no_assertion_evidence')}</div>
            ) : (
              result.assertions.map((observation) => {
                const assertion = assertionDefinitions.get(observation.assertion_id);
                return (
                  <div key={observation.assertion_id} className="flex items-start gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
                    <span className={cn('mt-1 h-2 w-2 shrink-0 rounded-full', observation.passed ? 'bg-green' : observation.severity === 'critical' ? 'bg-red' : 'bg-yellow')} />
                    <div className="min-w-0 flex-1">
                      <div className="text-xs font-strong text-tx-0">{assertion?.name ?? observation.assertion_id}</div>
                      <div className="mt-1 font-code text-type-micro text-tx-2">
                        {assertion ? `${assertion.source} ${operatorText(assertion)} · ` : ''}
                        {observation.actual ?? observation.message ?? '—'}
                      </div>
                    </div>
                    <span className="text-type-micro uppercase tracking-wide text-tx-3">{observation.severity}</span>
                  </div>
                );
              })
            )}
          </FormSection>

          <FormSection title={t('detail.request_response')}>
            <dl className="divide-y divide-bd-0 rounded-md border border-bd-0 bg-bg-2">
              {Object.entries(result.metadata).map(([key, value]) => (
                <div key={key} className="grid grid-cols-[minmax(120px,0.45fr)_minmax(0,1fr)] gap-3 px-3 py-2 text-xs">
                  <dt className="font-code text-tx-3">{key}</dt>
                  <dd className="break-all font-code text-tx-1">{value}</dd>
                </div>
              ))}
              {Object.keys(result.metadata).length === 0 && <div className="px-3 py-4 text-center text-xs text-tx-3">{t('detail.no_metadata')}</div>}
            </dl>
          </FormSection>

          {(result.outcome === 'failing' || result.outcome === 'degraded') && (
            <FormSection title={t('detail.correlation')}>
              <div className="grid gap-2 sm:grid-cols-2">
                {correlationLinks.map(([label, to]) => (
                  <Link key={to} to={to} className="flex min-h-11 items-center justify-between rounded-md border border-bd-0 bg-bg-2 px-3 text-sm font-strong text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3">
                    {label}
                    <ExternalLink aria-hidden className="h-3.5 w-3.5 text-tx-3" />
                  </Link>
                ))}
              </div>
            </FormSection>
          )}
        </>
      )}
    </FormDrawer>
  );
}

function Summary({ label, value }: { label: React.ReactNode; value: React.ReactNode }) {
  return <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-3"><div className="text-type-micro font-strong uppercase tracking-wide text-tx-3">{label}</div><div className="mt-2 text-sm font-strong text-tx-0">{value}</div></div>;
}

function Timing({ label, value }: { label: string; value: number | undefined }) {
  return <div><div className="text-type-micro uppercase tracking-wide text-tx-3">{label}</div><div className="mt-0.5 font-code text-tx-1">{formatDuration(value)}</div></div>;
}

function assertionMap(revision: MonitorRevision | undefined): Map<string, MonitorAssertion> {
  if (!revision || revision.spec.kind === 'heartbeat' || revision.spec.kind === 'icmp' || revision.spec.kind === 'tls' || revision.spec.kind === 'browser') return new Map();
  const assertions = revision.spec.kind === 'http'
    ? revision.spec.configuration.steps.flatMap((step) => step.assertions)
    : revision.spec.configuration.assertions;
  return new Map(assertions.map((assertion) => [assertion.id, assertion]));
}

function operatorText(assertion: MonitorAssertion): string {
  const operator = assertion.operator;
  if ('expected' in operator) return `${operator.operator} ${operator.expected}`;
  if ('pattern' in operator) return `${operator.operator} ${operator.pattern}`;
  return operator.operator;
}

function decodeExcerpt(bytes: number[]): string {
  try { return new TextDecoder().decode(new Uint8Array(bytes)); } catch { return bytes.join(' '); }
}

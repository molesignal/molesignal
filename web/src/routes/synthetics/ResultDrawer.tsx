import { Download, ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import {
  getResultArtifact,
  type MonitorAssertion,
  type MonitorRevision,
  type ProbeLocation,
  type SyntheticMonitor,
  type SyntheticResult,
  type SyntheticResultArtifact,
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
                <div className="mt-3">
                  <div className="mb-2 text-type-micro font-strong uppercase tracking-wide text-tx-3">{t('detail.step_evidence')}</div>
                  {attempt.evidence && attempt.evidence.length > 0 ? (
                    <div className="space-y-2">
                      {attempt.evidence.map((step, index) => (
                        <div key={`${step.step_id}-${index}`} className="rounded border border-bd-0 bg-bg-0 px-3 py-2">
                          <div className="flex items-center gap-2">
                            <StatePill state={step.outcome} compact />
                            <span className="min-w-0 flex-1 truncate text-xs font-strong text-tx-1">{step.name || step.step_id}</span>
                            <span className="font-code text-type-micro text-tx-3">{step.action}</span>
                            <span className="font-code text-type-micro text-tx-3">{formatDuration(Math.max(0, step.finished_at - step.started_at))}</span>
                          </div>
                          {(step.error_category || step.error_message) && (
                            <div className="mt-2 text-xs text-red-soft">
                              {step.error_category && <span className="mr-2 font-strong">{step.error_category}</span>}
                              {step.error_message}
                            </div>
                          )}
                          {Object.keys(step.metadata).length > 0 && (
                            <dl className="mt-2 grid gap-x-3 gap-y-1 text-type-micro sm:grid-cols-2">
                              {Object.entries(step.metadata).map(([key, value]) => (
                                <div key={key} className="grid grid-cols-[minmax(80px,0.4fr)_minmax(0,1fr)] gap-2">
                                  <dt className="truncate font-code text-tx-3">{key}</dt>
                                  <dd className="truncate font-code text-tx-2" title={value}>{value}</dd>
                                </div>
                              ))}
                            </dl>
                          )}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="rounded border border-dashed border-bd-1 px-3 py-3 text-center text-xs text-tx-3">{t('detail.no_step_evidence')}</div>
                  )}
                </div>
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

          <FormSection title={t('detail.artifacts')}>
            {result.artifacts && result.artifacts.length > 0 ? (
              <div className="space-y-3">
                {result.artifacts.map((artifact) => (
                  <ArtifactPreview key={artifact.id} resultId={result.id} artifact={artifact} />
                ))}
              </div>
            ) : (
              <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center text-xs text-tx-3">{t('detail.no_artifacts')}</div>
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

          {(result.outcome === 'failing'
            || result.outcome === 'degraded'
            || result.outcome === 'flaky') && (
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

function ArtifactPreview({ resultId, artifact }: { resultId: string; artifact: SyntheticResultArtifact }) {
  const { t } = useTranslation('synthetics');
  const [url, setUrl] = React.useState<string>();
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState(false);
  const mounted = React.useRef(true);
  const urlRef = React.useRef<string>();
  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, []);

  const load = async (): Promise<string | undefined> => {
    if (urlRef.current) return urlRef.current;
    setLoading(true);
    setError(false);
    try {
      const blob = await getResultArtifact(resultId, artifact.id);
      if (!mounted.current) return undefined;
      const objectUrl = URL.createObjectURL(blob);
      urlRef.current = objectUrl;
      setUrl(objectUrl);
      return objectUrl;
    } catch {
      if (mounted.current) setError(true);
      return undefined;
    } finally {
      if (mounted.current) setLoading(false);
    }
  };

  const download = async () => {
    const artifactUrl = await load();
    if (!artifactUrl) return;
    const anchor = document.createElement('a');
    anchor.href = artifactUrl;
    anchor.download = artifact.kind === 'screenshot'
      ? `${artifact.name || 'screenshot'}.png`
      : artifact.kind === 'har'
        ? `${artifact.name || 'journey'}.har`
        : `${artifact.name || 'journey'}.trace.json`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  };
  return (
    <div className="overflow-hidden rounded-md border border-bd-0 bg-bg-2">
      <div className="flex items-center gap-3 px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-strong text-tx-1">{artifact.name || artifact.kind}</div>
          <div className="mt-0.5 font-code text-type-micro text-tx-3">{artifact.kind} · {formatBytes(artifact.content_length)}</div>
        </div>
        <button
          type="button"
          onClick={() => { void download(); }}
          disabled={loading}
          className="inline-flex min-h-8 items-center gap-1.5 rounded px-2 text-xs font-strong text-tx-2 hover:bg-bg-3 focus-visible:bg-bg-3 disabled:cursor-not-allowed disabled:text-tx-4"
          aria-label={t('detail.download_artifact')}
        >
          <Download aria-hidden className="h-3.5 w-3.5" />
          {t('detail.download')}
        </button>
      </div>
      {artifact.kind === 'screenshot' && url && (
        <img src={url} alt={t('detail.screenshot_alt', { name: artifact.name })} className="max-h-[520px] w-full border-t border-bd-0 bg-bg-0 object-contain" />
      )}
      {artifact.kind === 'screenshot' && !url && !error && !loading && (
        <button
          type="button"
          onClick={() => { void load(); }}
          className="w-full border-t border-bd-0 px-3 py-6 text-center text-xs font-strong text-tx-2 hover:bg-bg-3 focus-visible:bg-bg-3"
        >
          {t('detail.load_screenshot')}
        </button>
      )}
      {loading && <div className="border-t border-bd-0 px-3 py-6 text-center text-xs text-tx-3">{t('detail.loading_artifact')}</div>}
      {error && <div className="border-t border-bd-0 px-3 py-4 text-center text-xs text-red-soft">{t('detail.artifact_unavailable')}</div>}
    </div>
  );
}

function Timing({ label, value }: { label: string; value: number | undefined }) {
  return <div><div className="text-type-micro uppercase tracking-wide text-tx-3">{label}</div><div className="mt-0.5 font-code text-tx-1">{formatDuration(value)}</div></div>;
}

function assertionMap(revision: MonitorRevision | undefined): Map<string, MonitorAssertion> {
  if (!revision || revision.spec.kind === 'heartbeat' || revision.spec.kind === 'icmp' || revision.spec.kind === 'tls' || revision.spec.kind === 'ssh') return new Map();
  const assertions = revision.spec.kind === 'http'
    ? revision.spec.configuration.steps.flatMap((step) => step.assertions)
    : revision.spec.kind === 'browser'
      ? revision.spec.configuration.steps.flatMap((step) => step.action.action === 'assert' && step.action.assertion ? [step.action.assertion as MonitorAssertion] : [])
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

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

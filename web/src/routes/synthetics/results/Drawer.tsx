import { ExternalLink } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type {
  MonitorAssertion,
  MonitorRevision,
  ProbeAttempt,
  ProbeLocation,
  SyntheticMonitor,
  SyntheticResult,
  TimingBreakdown,
} from '@/api/synthetics';
import { formatMicrosActive } from '@/lib/time';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

import { OutcomeMark } from '../marks';
import { formatDuration, resultDurationMicros } from '../model';
import { STATE_COLOR } from '../summary';
import { ArtifactPreview } from './Artifact';
import { traceLink } from './traceroute';
import { TracerouteHops } from './TracerouteTable';

// The phases a probe can time on the way to a response, in the order they happen. Each value is
// the time spent in that phase alone, not a point on the way to the total.
const PHASES: ReadonlyArray<[label: string, key: keyof TimingBreakdown]> = [
  ['detail.timing_dns', 'dnsTime'],
  ['detail.timing_connect', 'connectTime'],
  ['detail.timing_tls', 'tlsTime'],
  ['detail.timing_first_byte', 'firstByteTime'],
];

/**
 * One run, as evidence: how it went, what it said went wrong, and what it left
 * behind.
 *
 * A run is opened to find out why it failed, so that comes first. Everything
 * after it is shown only when the run has it: an attempt without steps draws no
 * empty box for them, and a run without artifacts has no section saying so.
 */
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
  locations: readonly ProbeLocation[];
  onClose: () => void;
}) {
  const { t } = useTranslation('synthetics');
  const assertionDefinitions = assertionMap(revision);
  const metadata = result ? Object.entries(result.metadata) : [];

  return (
    <FormDrawer
      open={Boolean(result)}
      onOpenChange={(open) => !open && onClose()}
      title={monitor?.name ?? t('results.title')}
      subtitle={
        result
          ? `${formatMicrosActive(result.startedAt)} · ${locations.find((location) => location.id === result.locationId)?.name ?? result.locationId}`
          : undefined
      }
      width={720}
    >
      {result && (
        <>
          <dl
            data-result-facts
            className="mb-6 flex flex-wrap gap-x-[32px] gap-y-[12px] font-sans text-sm"
          >
            <Fact label={t('results.columns.outcome')}>
              <span className="inline-flex items-center gap-[10px]">
                <OutcomeMark outcome={result.outcome} />
                {result.isTest && <span className="text-xs text-tx-3">{t('results.test_run')}</span>}
              </span>
            </Fact>
            <Fact label={t('results.columns.duration')}>
              <span className="tabular-nums">{formatDuration(resultDurationMicros(result))}</span>
            </Fact>
            <Fact label={t('results.columns.attempts')}>
              <span className="tabular-nums">{result.attempts.length}</span>
            </Fact>
          </dl>

          {result.outcome === 'skipped' && result.metadata.skip_reason && (
            <p
              data-result-skip-reason={result.metadata.skip_reason}
              className="mb-6 rounded-md bg-bg-2 px-4 py-3 font-sans text-sm text-tx-1"
            >
              {t(`results.skip_reasons.${result.metadata.skip_reason}`, {
                defaultValue: result.metadata.skip_reason,
              })}
            </p>
          )}

          {result.attempts.length > 0 && (
            <FormSection title={t('detail.attempts')}>
              <ol className="divide-y divide-bd-0">
                {result.attempts.map((attempt) => (
                  <Attempt key={attempt.number} attempt={attempt} />
                ))}
              </ol>
              {result.attempts.some((attempt) =>
                attempt.evidence?.some((step) => step.metadata.trace_id),
              ) && (
                <p className="font-sans text-xs leading-relaxed text-tx-2">{t('results.trace_hint')}</p>
              )}
            </FormSection>
          )}

          {result.assertions.length > 0 && (
            <FormSection title={t('detail.assertion_evidence')}>
              <ul className="divide-y divide-bd-0">
                {result.assertions.map((observation) => {
                  const assertion = assertionDefinitions.get(observation.assertionId);

                  return (
                    <li key={observation.assertionId} className="flex items-start gap-[10px] py-[10px]">
                      <span
                        aria-hidden="true"
                        className="mt-[6px] h-[7px] w-[7px] shrink-0 rounded-full"
                        style={{
                          background: observation.passed
                            ? 'var(--green)'
                            : observation.severity === 'critical'
                              ? 'var(--red)'
                              : 'var(--yellow)',
                        }}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="font-sans text-sm font-strong text-tx-0">
                          {assertion?.name ?? observation.assertionId}
                        </div>
                        <div className="mt-[2px] break-words font-code text-xs text-tx-2">
                          {assertion ? `${assertion.source} ${operatorText(assertion)} · ` : ''}
                          {observation.actual ?? observation.message ?? '—'}
                        </div>
                      </div>
                      <span className="shrink-0 font-sans text-xs text-tx-2">
                        {t(`detail.severity.${observation.severity}`)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </FormSection>
          )}

          {result.artifacts && result.artifacts.length > 0 && (
            <FormSection title={t('detail.artifacts')}>
              <ul className="divide-y divide-bd-0">
                {result.artifacts.map((artifact) => (
                  <ArtifactPreview key={artifact.id} resultId={result.id} artifact={artifact} />
                ))}
              </ul>
            </FormSection>
          )}

          {metadata.length > 0 && (
            <FormSection title={t('detail.request_response')}>
              <dl className="divide-y divide-bd-0">
                {metadata.map(([key, value]) => (
                  <div
                    key={key}
                    className="grid grid-cols-[minmax(120px,0.45fr)_minmax(0,1fr)] gap-3 py-[8px] text-xs"
                  >
                    <dt className="font-code text-tx-2">{key}</dt>
                    <dd className="break-all font-code text-tx-0">{value}</dd>
                  </div>
                ))}
              </dl>
            </FormSection>
          )}
        </>
      )}
    </FormDrawer>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-tx-2">{label}</dt>
      <dd className="mt-[4px] text-tx-0">{children}</dd>
    </div>
  );
}

/** One try of the run: how it went, what it said, and the steps it took when it has steps. */
function Attempt({ attempt }: { attempt: ProbeAttempt }) {
  const { t } = useTranslation('synthetics');
  const phases = PHASES.filter(([, key]) => attempt.timing[key] !== undefined);
  const excerpt = attempt.boundedResponseExcerpt;
  const steps = attempt.evidence ?? [];

  return (
    <li data-attempt={attempt.number} className="py-[12px] first:pt-0">
      <div className="flex items-center gap-[12px]">
        <span className="font-sans text-sm font-strong text-tx-0">
          {t('detail.attempt', { number: attempt.number })}
        </span>
        <OutcomeMark outcome={attempt.outcome} />
        <span className="ml-auto font-sans text-xs tabular-nums text-tx-1">
          {formatDuration(attempt.timing.totalTime ?? attempt.finishedAt - attempt.startedAt)}
        </span>
      </div>

      {(attempt.errorMessage || attempt.errorCategory) && (
        <p
          role="alert"
          className="mt-[8px] break-words rounded-md bg-red-dim px-[12px] py-[8px] font-sans text-xs leading-relaxed text-red"
        >
          {attempt.errorCategory && (
            <span className="mr-[8px] font-code font-strong">{attempt.errorCategory}</span>
          )}
          {attempt.errorMessage}
        </p>
      )}

      {/* A probe reports only the phases it timed, so a run shows the ones it has. */}
      {attempt.metadata?.['rum.session_id'] && (
        <Link
          data-result-rum-session={attempt.metadata['rum.session_id']}
          to={`/rum/sessions/view/${encodeURIComponent(attempt.metadata['rum.session_id'])}?${new URLSearchParams({
            from: new Date(attempt.startedAt / 1_000 - 60_000).toISOString(),
            to: new Date(attempt.finishedAt / 1_000 + 60_000).toISOString(),
          })}`}
          className="mt-[8px] inline-block font-sans text-xs text-[var(--blue-ink)] outline-none underline-offset-4 hover:underline focus-visible:underline"
        >
          {t('detail.open_rum_session')}
        </Link>
      )}

      {phases.length > 0 && (
        <dl className="mt-[10px] flex flex-wrap gap-x-[24px] gap-y-[6px] font-sans text-xs">
          {phases.map(([label, key]) => (
            <div key={key}>
              <dt className="text-tx-2">{t(label)}</dt>
              <dd className="mt-[2px] tabular-nums text-tx-0">{formatDuration(attempt.timing[key])}</dd>
            </div>
          ))}
        </dl>
      )}

      {steps.length > 0 && (
        <ol className="mt-[10px] space-y-[6px]">
          {steps.map((step, index) => (
            <li
              key={`${step.stepId}-${index}`}
              className="rounded-md bg-bg-2 px-[12px] py-[8px]"
            >
              <div className="flex min-w-0 items-center gap-[8px]">
                <span
                  aria-hidden="true"
                  className="h-[7px] w-[7px] shrink-0 rounded-full"
                  style={{ background: STATE_COLOR[step.outcome] }}
                />
                <span className="min-w-0 flex-1 truncate font-sans text-xs font-strong text-tx-0">
                  {step.action === 'traceroute' && step.metadata.target
                    ? t('results.traceroute.title', { target: step.metadata.target })
                    : step.name || step.stepId}
                </span>
                {step.metadata.trace_id && (
                  <Link
                    to={traceLink(step.metadata.trace_id, step.startedAt, step.finishedAt)}
                    className="inline-flex shrink-0 items-center gap-1 rounded font-sans text-xs text-indigo-soft hover:text-tx-0 focus-visible:bg-bg-3"
                  >
                    {t('results.view_trace')}
                    <ExternalLink aria-hidden className="h-3 w-3 shrink-0" />
                  </Link>
                )}
                <span className="shrink-0 font-code text-xs text-tx-2">{step.action}</span>
                <span className="shrink-0 font-sans text-xs tabular-nums text-tx-1">
                  {formatDuration(Math.max(0, step.finishedAt - step.startedAt))}
                </span>
              </div>
              {(step.errorCategory || step.errorMessage) && (
                <p className="mt-[6px] break-words font-sans text-xs text-red">
                  {step.errorCategory && (
                    <span className="mr-[8px] font-code font-strong">{step.errorCategory}</span>
                  )}
                  {step.errorMessage}
                </p>
              )}
              {step.action === 'traceroute' && <TracerouteHops step={step} />}
              {step.action !== 'traceroute' && Object.keys(step.metadata).length > 0 && (
                <dl className="mt-[6px] grid gap-x-3 gap-y-1 text-xs sm:grid-cols-2">
                  {Object.entries(step.metadata).map(([key, value]) => (
                    <div
                      key={key}
                      className={cn('grid grid-cols-[minmax(80px,0.4fr)_minmax(0,1fr)] gap-2')}
                    >
                      <dt className="truncate font-code text-tx-2">{key}</dt>
                      <dd className="truncate font-code text-tx-1" title={value}>
                        {value}
                      </dd>
                    </div>
                  ))}
                </dl>
              )}
            </li>
          ))}
        </ol>
      )}

      {excerpt && excerpt.length > 0 && (
        <pre className="mt-[10px] max-h-56 overflow-auto rounded-md bg-bg-2 p-[12px] font-code text-xs leading-relaxed text-tx-1">
          {decodeExcerpt(excerpt)}
        </pre>
      )}
    </li>
  );
}

function assertionMap(revision: MonitorRevision | undefined): Map<string, MonitorAssertion> {
  if (
    !revision ||
    revision.spec.kind === 'heartbeat' ||
    revision.spec.kind === 'icmp' ||
    revision.spec.kind === 'tls' ||
    revision.spec.kind === 'ssh'
  ) {
    return new Map();
  }

  const assertions =
    revision.spec.kind === 'http'
      ? revision.spec.configuration.steps.flatMap((step) => step.assertions)
      : revision.spec.kind === 'browser'
        ? revision.spec.configuration.steps.flatMap((step) =>
            step.action.action === 'assert' && step.action.assertion
              ? [step.action.assertion as MonitorAssertion]
              : [],
          )
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
  try {
    return new TextDecoder().decode(new Uint8Array(bytes));
  } catch {
    return bytes.join(' ');
  }
}

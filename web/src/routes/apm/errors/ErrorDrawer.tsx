import { useQuery } from '@tanstack/react-query';
import { Activity, Combine, ExternalLink, FileCode2, ListTree, Route } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { useTimeFormatter } from '@/lib/time';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatCount } from '../format';
import { ErrorBreakdownCard } from './Breakdown';
import { type ErrorRulesView, ErrorRulesSheet } from './ErrorRules';
import { IssueBadge } from './IssueBadge';
import { IssueControls } from './IssueControls';
import { emptyRule, ruleFromGroup } from './rules';
import { ApmDrawer } from '../frame/ApmDrawer';
import { DrawerTrail, useTrailLabel } from '../frame/DrawerTrail';
import { InvestigateButton } from '../frame/investigate';
import { ApmCard, ApmQueryState } from '../frame/Page';
import { useTraceOpener } from '../frame/TraceDrawerHost';
import { useAddressedDrawer, useApmScope } from '../frame/useApmScope';
import { apmQueryParams, OTHER_DIMENSION, servicePath } from '../scope';
import { signalHref, traceLogsHref } from '../service/links';

const LINK = 'inline-flex items-center gap-1 font-strong text-tx-2 hover:text-tx-0';

/**
 * An error group opened over the page, as Datadog opens an issue: its state
 * and who has it, how often it happened over time and where (versions,
 * environments, languages), where it was thrown, the endpoints it touched,
 * and samples whose traces open in place, each with its logs. The group's
 * logs and a rule for errors like it are a click from its facts.
 */
export function ErrorDrawer() {
  const { t } = useTranslation('apm');
  const time = useTimeFormatter();
  const drawer = useAddressedDrawer('error');
  const { scope, range, orgId, searchParams } = useApmScope();
  const { openTrace } = useTraceOpener();
  const fingerprint = drawer.value;
  const params = apmQueryParams(scope, range);
  const [rulesView, setRulesView] = React.useState<ErrorRulesView | null>(null);

  const detail = useQuery({
    queryKey: apmQueryKeys.error(orgId, fingerprint ?? '', params),
    queryFn: () => apmApi.error(fingerprint ?? '', params),
    enabled: Boolean(orgId && fingerprint),
  });

  const data = detail.data;
  const group = data?.group;
  useTrailLabel('error', group?.error.errorType);

  return (
    <ApmDrawer
      open={Boolean(fingerprint)}
      onClose={() => drawer.close()}
      testId="error"
      trail={<DrawerTrail over="error" />}
      title={group ? `${group.error.errorType}` : t('errors.title')}
      subtitle={group ? `${group.service.name} · ${group.service.environment}` : undefined}
      actions={
        group ? (
          <>
            <IssueControls group={group} />
            <InvestigateButton
              subject={{
                service: group.service,
                range,
                error: { type: group.error.errorType, fingerprint: group.error.fingerprint },
              }}
            />
          </>
        ) : undefined
      }
    >
      <ApmQueryState query={detail} empty={false}>
        {data && group && (
          <>
            <div className="flex flex-wrap items-center gap-x-4 gap-y-1 font-sans text-xs text-tx-2">
              <IssueBadge issue={group.issue} />
              <span>
                {t('errors.facts.occurrences')} <span className="tabular-nums text-tx-0">{formatCount(group.occurrenceCount)}</span>
              </span>
              <span>
                {t('errors.facts.first_seen')} <span className="text-tx-0">{time.micros(group.firstSeenAt, false)}</span>
              </span>
              <span>
                {t('errors.facts.last_seen')} <span className="text-tx-0">{time.micros(group.lastSeenAt, false)}</span>
              </span>
              {data.affectedVersions.length > 0 && (
                <span>
                  {t('errors.facts.versions')} <span className="text-tx-0">{data.affectedVersions.join(', ')}</span>
                </span>
              )}
              <span className="ml-auto flex items-center gap-4">
                <Link to={signalHref('logs', group.traces)} className={LINK} data-apm-error-logs="">
                  {t('errors.logs')}
                  <ExternalLink aria-hidden className="h-3 w-3" />
                </Link>
                <button type="button" className={LINK} onClick={() => setRulesView({ mode: 'form', rule: null, initial: ruleFromGroup(group) })} data-apm-error-rule-from="">
                  <Combine aria-hidden className="h-3 w-3 text-[var(--blue-ink)]" />
                  {t('errors.rules.from_error')}
                </button>
              </span>
            </div>
            {group.representativeMessage && <p className="whitespace-pre-wrap break-words font-sans text-sm text-tx-0">{group.representativeMessage}</p>}
            <ApmCard icon={Activity} tone="text-[var(--red-ink)]" title={t('errors.trend')} testId="error-trend" bodyClassName="px-2 pb-2 pt-1">
              <TimeSeriesChart
                series={[
                  {
                    id: 'occurrences',
                    name: t('errors.facts.occurrences'),
                    timestamps: data.trend.map((point) => point.bucketAt),
                    data: data.trend.map((point) => point.red.requestCount),
                    color: 'var(--red)',
                  },
                ]}
                xDomain={[data.meta.range.from, data.meta.range.to]}
                height={140}
                cursorScopeId={null}
                ariaLabel={t('errors.trend')}
                showLegend={false}
                options={{
                  drawStyle: 'bar',
                  showPoints: 'never',
                  compactAxes: true,
                  leftAxis: { scale: 'linear', min: 0 },
                }}
              />
            </ApmCard>
            {data.breakdown && <ErrorBreakdownCard breakdown={data.breakdown} testId="error-breakdown" />}
            {data.representativeStack.length > 0 && (
              <ApmCard icon={FileCode2} tone="text-[var(--chart-6)]" title={t('errors.stack')} testId="error-stack">
                <ol className="max-h-72 overflow-auto px-3 py-2 font-sans text-xs leading-5 text-tx-1">
                  {data.representativeStack.map((frame, index) => (
                    <li key={`${index}-${frame}`} className="whitespace-pre-wrap break-all">
                      {frame}
                    </li>
                  ))}
                </ol>
              </ApmCard>
            )}
            <div className="grid min-w-0 gap-3 xl:grid-cols-2">
              <ApmCard icon={Route} tone="text-[var(--blue-ink)]" title={t('errors.endpoints')} testId="error-endpoints">
                {data.affectedTransactions.length === 0 ? (
                  <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('errors.no_endpoints')}</p>
                ) : (
                  data.affectedTransactions.map((item) => {
                    // The error stays open under the endpoint, its way back.
                    const href = `${servicePath(item.service, 'endpoints', searchParams)}&${new URLSearchParams({ error: fingerprint ?? '', endpoint: item.transaction.name, kind: item.transaction.kind })}`;
                    return (
                      <Link
                        key={`${item.transaction.kind}:${item.transaction.name}`}
                        to={href}
                        className="flex h-8 items-center gap-2 border-b border-bd-0 px-3 font-sans text-xs text-tx-0 last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
                      >
                        {item.transaction.name === OTHER_DIMENSION ? (
                          <span className="truncate text-tx-3">{t('overflow.other')}</span>
                        ) : (
                          <TraceOperationName operation={item.transaction.name} className="min-w-0" />
                        )}
                        <span className="ml-auto shrink-0 tabular-nums text-tx-2">{formatCount(item.red.errorCount)}</span>
                      </Link>
                    );
                  })
                )}
              </ApmCard>
              <ApmCard icon={ListTree} tone="text-[var(--chart-1)]" title={t('errors.samples')} description={t('errors.samples_description')} testId="error-samples">
                {data.samples.length === 0 ? (
                  <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('errors.no_samples')}</p>
                ) : (
                  data.samples.map((sample) => (
                    <div key={`${sample.traceId}:${sample.spanId}`} className="flex h-8 items-center border-b border-bd-0 last:border-b-0 hover:bg-bg-2">
                      <button
                        type="button"
                        data-apm-error-sample={sample.traceId}
                        onClick={(event) =>
                          openTrace(
                            {
                              traceId: sample.traceId,
                              spanId: sample.spanId,
                              window: {
                                from: sample.eventTime - 3_600_000_000,
                                to: sample.eventTime + 3_600_000_000,
                              },
                            },
                            event.currentTarget,
                          )
                        }
                        className="grid h-8 min-w-0 flex-1 grid-cols-[150px_minmax(0,1fr)_72px] items-center gap-3 pl-3 text-left font-sans text-xs outline-none focus-visible:bg-bg-2"
                      >
                        <span className="truncate tabular-nums text-tx-2">{time.micros(sample.eventTime)}</span>
                        <span className="truncate text-tx-1">{sample.representativeMessage ?? ''}</span>
                        <span className="truncate text-right tabular-nums text-tx-3" title={sample.traceId}>
                          {sample.traceId.slice(0, 8)}
                        </span>
                      </button>
                      <Link
                        to={traceLogsHref(sample.traceId, sample.eventTime)}
                        data-apm-error-sample-logs={sample.traceId}
                        className="shrink-0 px-3 font-sans text-xs font-strong text-tx-2 outline-none hover:text-tx-0 focus-visible:text-tx-0"
                      >
                        {t('errors.sample_logs')}
                      </Link>
                    </div>
                  ))
                )}
              </ApmCard>
            </div>
          </>
        )}
      </ApmQueryState>
      <ErrorRulesSheet view={rulesView} onView={setRulesView} groups={group ? [group] : []} newRule={() => (group ? ruleFromGroup(group) : emptyRule())} />
    </ApmDrawer>
  );
}

import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  CircleX,
  Gauge,
  Globe2,
  MousePointerClick,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { SessionRow } from '@/api/rum';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import { EventTimeline, normalizePlayerEvents } from './replay/events';
import { ReplayPlayer } from './replay/ReplayPlayer';
import { RumDetailPage, RumSectionHeader, useRumBasePath } from './RumLayout';

export function SessionDetail() {
  const { t } = useTranslation('rum');
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => windowToMicros(window), [window]);

  const sessionQuery = useQuery({
    queryKey: ['rum', 'session', orgId, id, range.from_micros, range.to_micros],
    queryFn: () => rumApi.getSession({ org_id: orgId, session_id: id, ...range }),
    enabled: !!orgId && !!id,
  });
  const replayQuery = useQuery({
    queryKey: ['rum', 'replay', orgId, id],
    queryFn: () => rumApi.getReplay(id),
    enabled: !!orgId && !!id,
    retry: false,
  });
  const tracesQuery = useQuery({
    queryKey: ['rum', 'related-traces', orgId, id],
    queryFn: () => rumApi.relatedTraces(id),
    enabled: !!orgId && !!id,
    retry: false,
  });

  const session = sessionQuery.data?.session;
  const timelineEvents = React.useMemo(
    () =>
      normalizePlayerEvents(
        replayQuery.data?.events ?? [],
        sessionQuery.data?.events ?? [],
      ),
    [replayQuery.data?.events, sessionQuery.data?.events],
  );
  const state = queryStateFor({
    isLoading: sessionQuery.isLoading,
    isError: sessionQuery.isError,
    data: session,
  });
  const pageState = productStateFor(state, {
    error: sessionQuery.error,
    emptyTitle: t('session_detail.no_events'),
  });
  const sessionWindow = sessionTimeWindow(session);

  return (
    <RumDetailPage
      title={sessionTitle(session, t)}
      subtitle={id}
      toolbar={
        <>
          {session && (
            <Pill tone={experienceTone(session.experience)}>
              {t(`experience.${session.experience}`)}
            </Pill>
          )}
          <ChromeButton onClick={() => navigate(`${basePath}/sessions`)}>
            <ChevronLeft className="h-4 w-4" />
            {t('session_detail.back')}
          </ChromeButton>
        </>
      }
      state={pageState}
      bodyClassName="pt-5"
    >
      {session && (
        <div className="grid min-h-0 gap-5 xl:grid-cols-[minmax(0,1fr)_320px]">
          <div className="min-w-0">
            <ReplayPlayer
              replayEvents={replayQuery.data?.events ?? []}
              timelineEvents={timelineEvents}
              session={session}
              loading={replayQuery.isLoading}
              loadError={replayQuery.error}
            />
          </div>

          <aside className="min-w-0 border-l-0 border-bd-0 xl:border-l xl:pl-5">
            <SessionFacts session={session} />
          </aside>

          <section className="min-w-0 xl:col-span-2">
            <RumSectionHeader
              title={t('session_detail.event_timeline')}
              description={t('session_detail.event_timeline_description')}
            />
            <EventTimeline events={timelineEvents} sessionStart={session.started_at_micros} />
          </section>

          <section className="min-w-0 xl:col-span-2">
            <RumSectionHeader
              title={t('session_detail.related_traces_title')}
              description={t('session_detail.related_traces_subtitle')}
              action={
                tracesQuery.data?.primary_service ? (
                  <span className="text-xs text-tx-2">
                    {t('session_detail.primary_service_label')}:{' '}
                    <SignalReference
                      type="service"
                      value={tracesQuery.data.primary_service}
                      time={sessionWindow}
                    >
                      {tracesQuery.data.primary_service}
                    </SignalReference>
                  </span>
                ) : null
              }
            />
            <RelatedTraces
              rows={tracesQuery.data?.traces ?? []}
              sessionWindow={sessionWindow}
            />
          </section>
        </div>
      )}
    </RumDetailPage>
  );
}

function SessionFacts({ session }: { session: SessionRow }) {
  const { t } = useTranslation('rum');
  const issueTotal = sessionIssueCount(session);
  return (
    <div className="space-y-5">
      <section>
        <RumSectionHeader title={t('session_detail.user_context')} />
        <dl className="m-0 mt-3 grid grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-2.5 text-xs">
          <Fact label={t('sessions.columns.user')} value={session.user_id ?? t('sessions.anonymous_user')} />
          <Fact
            label={t('session_detail.device')}
            value={[session.browser, session.os ?? session.device].filter(Boolean).join(' · ') || '—'}
          />
          <Fact label={t('sessions.columns.country')} value={session.country ?? '—'} />
          <Fact label={t('session_detail.ip_address')} value={session.ip_address ?? '—'} />
          <Fact label={t('scope.application')} value={session.application ?? '—'} />
          <Fact label={t('scope.environment')} value={session.environment ?? '—'} />
          <Fact label={t('scope.version')} value={session.version ?? '—'} />
          <Fact label={t('sessions.columns.duration')} value={formatDurationMs(session.duration_ms)} />
        </dl>
      </section>

      <section>
        <RumSectionHeader title={t('session_detail.current_page')} />
        <div className="mt-3 rounded-md bg-bg-2 p-3">
          <div className="flex items-center gap-2">
            <Globe2 className="h-4 w-4 text-blue-soft" />
            <span className="min-w-0 truncate font-mono text-xs font-strong text-tx-0">
              {session.last_page ?? session.journey[session.journey.length - 1] ?? '—'}
            </span>
          </div>
          {session.journey.length > 1 && (
            <div className="mt-3 flex min-w-0 items-center gap-1 overflow-hidden text-xs text-tx-3">
              {session.journey.slice(0, 4).map((page, index) => (
                <React.Fragment key={`${page}-${index}`}>
                  {index > 0 && <ChevronRight className="h-3 w-3 shrink-0" />}
                  <span className="min-w-0 truncate">{page}</span>
                </React.Fragment>
              ))}
            </div>
          )}
        </div>
      </section>

      <section>
        <RumSectionHeader title={t('session_detail.detected_issues')} />
        <div className="mt-3 space-y-2">
          {issueTotal === 0 ? (
            <div className="rounded-md bg-green-dim px-3 py-2.5 text-xs font-strong text-green-soft">
              {t('session_detail.no_detected_issues')}
            </div>
          ) : (
            <>
              <IssueFact
                icon={<AlertTriangle className="h-4 w-4" />}
                label={t('sessions.issue.error')}
                value={(session.error_count ?? 0) + session.failed_request_count}
                tone="red"
              />
              <IssueFact
                icon={<MousePointerClick className="h-4 w-4" />}
                label={t('sessions.issue.rage_click')}
                value={session.rage_click_count}
                tone="yellow"
              />
              <IssueFact
                icon={<CircleX className="h-4 w-4" />}
                label={t('sessions.issue.dead_click')}
                value={session.dead_click_count}
                tone="yellow"
              />
              <IssueFact
                icon={<Gauge className="h-4 w-4" />}
                label={t('sessions.issue.slow')}
                value={session.slow_resource_count}
                tone="yellow"
              />
            </>
          )}
        </div>
      </section>
    </div>
  );
}

function Fact({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <>
      <dt className="text-tx-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate font-strong text-tx-1">{value}</dd>
    </>
  );
}

function IssueFact({
  icon,
  label,
  value,
  tone,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
  tone: 'red' | 'yellow';
}) {
  if (value <= 0) return null;
  return (
    <div
      className={cn(
        'flex items-center gap-2 rounded-md px-3 py-2.5 text-xs font-strong',
        tone === 'red' ? 'bg-red-dim text-red-soft' : 'bg-yellow-dim text-yellow-soft',
      )}
    >
      {icon}
      <span>{label}</span>
      <span className="ml-auto font-mono">{value}</span>
    </div>
  );
}

function RelatedTraces({
  rows,
  sessionWindow,
}: {
  rows: rumApi.RelatedTraceRow[];
  sessionWindow: { from: string; to: string } | undefined;
}) {
  const { t } = useTranslation('rum');
  if (rows.length === 0) {
    return <div className="grid min-h-32 place-items-center text-sm text-tx-3">{t('session_detail.related_empty')}</div>;
  }
  return (
    <div className="grid gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
      {rows.map((row) => (
        <div
          key={row.trace_id}
          className="grid min-h-[72px] grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-bd-0 py-3"
        >
          <span className="min-w-0">
            <SignalReference type="trace_id" value={row.trace_id} time={sessionWindow}>
              {shortTraceId(row.trace_id)}
            </SignalReference>
            <span className="mt-1 block truncate text-xs text-tx-3">
              {row.service ?? '—'} · {row.span_count} {t('session_detail.related_columns.spans')}
            </span>
          </span>
          <span className="text-right">
            <span className="block font-mono text-xs font-strong text-tx-1">
              {row.duration_ms != null ? `${Math.round(row.duration_ms)} ms` : '—'}
            </span>
            <Pill tone={row.relation === 'direct' ? 'green' : 'yellow'}>
              {t(
                row.relation === 'direct'
                  ? 'session_detail.relation_direct'
                  : 'session_detail.relation_time_correlated',
              )}
            </Pill>
          </span>
        </div>
      ))}
    </div>
  );
}

function sessionTimeWindow(
  session: SessionRow | null | undefined,
): { from: string; to: string } | undefined {
  if (!session?.started_at_micros) return undefined;
  const start = new Date(Math.floor(session.started_at_micros / 1_000)).toISOString();
  const end = new Date(
    Math.floor((session.started_at_micros + (session.duration_ms ?? 0) * 1_000) / 1_000),
  ).toISOString();
  return { from: start, to: end };
}

function sessionTitle(
  session: SessionRow | null | undefined,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  if (!session) return t('session_detail.title');
  const issue =
    (session.error_count ?? 0) + session.failed_request_count > 0
      ? t('session_detail.title_error')
      : session.rage_click_count > 0
        ? t('session_detail.title_rage_click')
        : t('session_detail.title');
  return `${issue} · ${session.last_page ?? session.journey[0] ?? '—'}`;
}

function experienceTone(grade: rumApi.ExperienceGrade): PillTone {
  if (grade === 'good') return 'green';
  if (grade === 'needs_improvement') return 'yellow';
  if (grade === 'poor') return 'red';
  return 'dim';
}

function sessionIssueCount(session: SessionRow): number {
  return (
    (session.error_count ?? 0) +
    session.failed_request_count +
    session.rage_click_count +
    session.dead_click_count +
    session.slow_resource_count +
    session.crash_count
  );
}

function shortTraceId(traceId: string): string {
  return traceId.length > 14 ? `${traceId.slice(0, 12)}…` : traceId;
}

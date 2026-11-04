import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useSearchParams } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { RelatedTrace } from '@/api/rum';
import { productStateFor } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { SignalReference } from '@/shell/SignalReference';
import { useAuthStore } from '@/stores/auth';

import { CopyMomentLink } from './CopyMomentLink';
import { useApplicationRef } from '../agent/useApplication';
import { DevTools } from './devtools/DevTools';
import { replayMarks } from './devtools/model';
import { SessionFacts } from './Facts';
import { addressRange, addressReplayAt, sessionHeadline, sessionPage, sessionWindow } from './model';
import { PlaylistBar } from './PlaylistBar';
import { PlaylistMenu } from './PlaylistMenu';
import { SessionSummary } from './summary/SessionSummary';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import { ReplayController } from '../replay/controller';
import { EventTimeline, normalizePlayerEvents } from '../replay/events';
import { ReplayPlayer } from '../replay/ReplayPlayer';
import { RumDetailPage, RumSectionHeader } from '../RumLayout';

/**
 * One session: the replay and the tools under it, who it was of, what it ran into, everything it did in
 * order, and the traces it made. Another session is another page: nothing of the one before stays.
 */
export function SessionDetail() {
  const { id = '' } = useParams<{ id: string }>();

  return <SessionPage key={id} id={id} />;
}

function SessionPage({ id }: { id: string }) {
  const { t, i18n } = useTranslation('rum');
  const [searchParams] = useSearchParams();
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const range = React.useMemo(() => addressRange(searchParams), [searchParams]);
  // Where the replay opens, from the address: a moment, held there until the person plays it.
  const startAt = React.useMemo(() => addressReplayAt(searchParams), [searchParams]);
  // The playlist the session is gone through in, if it was opened from one.
  const playlistId = searchParams.get('playlist') ?? '';
  // What lets the tools under the replay read where it is and move it.
  const [controller] = React.useState(() => new ReplayController());

  const timelineQuery = useQuery({
    queryKey: ['rum', 'session', orgId, id, range?.from, range?.to],
    queryFn: ({ signal }) => rumApi.timeline(id, range, signal),
    enabled: Boolean(orgId && id),
  });
  const replayQuery = useQuery({
    queryKey: ['rum', 'replay', orgId, id],
    queryFn: () => rumApi.replay(id),
    enabled: Boolean(orgId && id),
    retry: false,
  });
  const tracesQuery = useQuery({
    queryKey: ['rum', 'related-traces', orgId, id, range?.from, range?.to],
    queryFn: () => rumApi.relatedTraces(id, range),
    enabled: Boolean(orgId && id),
    retry: false,
  });

  const commentsKey = React.useMemo(() => ['rum', 'comments', orgId, id] as const, [id, orgId]);
  const commentsQuery = useQuery({
    queryKey: commentsKey,
    queryFn: ({ signal }) => rumApi.comments(id, signal),
    enabled: Boolean(orgId && id),
    retry: false,
  });
  const summaryKey = React.useMemo(() => ['rum', 'summary', orgId, id] as const, [id, orgId]);
  const summaryQuery = useQuery({
    queryKey: summaryKey,
    queryFn: ({ signal }) => rumApi.sessionSummary(id, signal),
    enabled: Boolean(orgId && id),
    retry: false,
  });

  const session = timelineQuery.data?.session ?? null;
  const timelineEvents = React.useMemo(
    () => normalizePlayerEvents(replayQuery.data?.events ?? [], timelineQuery.data?.items ?? []),
    [replayQuery.data?.events, timelineQuery.data?.items],
  );
  const state = productStateFor(
    queryStateFor({ isLoading: timelineQuery.isLoading, isError: timelineQuery.isError, data: session }),
    { error: timelineQuery.error, emptyTitle: t('session_detail.not_found') },
  );
  const items = timelineQuery.data?.items;
  const comments = commentsQuery.data;
  const chapters = summaryQuery.data?.chapters;
  const marks = React.useMemo(() => replayMarks(items ?? [], comments, chapters), [chapters, comments, items]);
  const during = sessionWindow(session);
  const duration = typeof session?.['session.duration'] === 'number' ? session['session.duration'] : undefined;
  const application = useApplicationRef(typeof session?.['application.id'] === 'string' ? session['application.id'] : '');

  return (
    <RumDetailPage
      title={session ? title(session, t) : t('session_detail.title')}
      subtitle={id}
      toolbar={
        session ? (
          <>
            {session['session.has_replay'] === true && <Pill tone="green">{t('session_detail.replay_recorded')}</Pill>}
            <CopyMomentLink id={id} session={session} controller={controller} />
            {application.id !== '' && <PlaylistMenu id={id} session={session} application={application.id} controller={controller} />}
            {application.id !== '' && during && (
              <RumInvestigateButton
                label={t('agent.investigate_session')}
                title={t('agent.investigate_title')}
                subject={{
                  application,
                  environment: typeof session['env'] === 'string' ? session['env'] : '',
                  range: { from: Date.parse(during.from) * 1_000, to: Date.parse(during.to) * 1_000 },
                  focus: { kind: 'session', id },
                }}
              />
            )}
          </>
        ) : undefined
      }
      lead={playlistId !== '' ? <PlaylistBar playlistId={playlistId} current={id} /> : undefined}
      state={state}
      bodyClassName="pt-5"
    >
      {session && (
        <div className="grid min-h-0 gap-5 xl:grid-cols-[minmax(0,1fr)_320px]">
          <div className="min-w-0 space-y-5">
            <ReplayPlayer
              replayEvents={replayQuery.data?.events ?? []}
              timelineEvents={timelineEvents}
              durationNs={duration}
              loading={replayQuery.isLoading}
              loadError={replayQuery.error}
              controller={controller}
              marks={marks}
              startAt={startAt}
            />
            <DevTools
              controller={controller}
              items={items ?? []}
              truncated={timelineQuery.data?.truncated ?? []}
              application={typeof session['application.id'] === 'string' ? session['application.id'] : ''}
              sessionStart={session.timestamp}
              hasReplay={session['session.has_replay'] === true}
              language={i18n.resolvedLanguage ?? i18n.language}
              session={id}
              comments={comments ?? []}
              commentsKey={commentsKey}
            />
          </div>

          <aside className="min-w-0 space-y-5 border-l-0 border-bd-0 xl:border-l xl:pl-5">
            <SessionSummary
              session={id}
              summary={summaryQuery.data}
              loading={summaryQuery.isLoading}
              range={range}
              controller={controller}
              zero={session.timestamp}
              queryKey={summaryKey}
            />
            <SessionFacts row={session} />
          </aside>

          <section className="min-w-0 xl:col-span-2">
            <RumSectionHeader
              title={t('session_detail.event_timeline')}
              description={
                timelineQuery.data?.truncated.length
                  ? t('session_detail.timeline_truncated', {
                      types: timelineQuery.data.truncated.map((type) => t(`explorer.types.${type}`)).join(', '),
                    })
                  : t('session_detail.event_timeline_description')
              }
            />
            <EventTimeline events={timelineEvents} sessionStart={session.timestamp} />
          </section>

          <section className="min-w-0 xl:col-span-2">
            <RumSectionHeader
              title={t('session_detail.related_traces_title')}
              description={t('session_detail.related_traces_subtitle')}
              action={
                tracesQuery.data?.primaryService ? (
                  <span className="text-xs text-tx-2">
                    {t('session_detail.primary_service_label')}:{' '}
                    <SignalReference type="service" value={tracesQuery.data.primaryService} time={during}>
                      {tracesQuery.data.primaryService}
                    </SignalReference>
                  </span>
                ) : null
              }
            />
            <RelatedTraces rows={tracesQuery.data?.traces ?? []} during={during} />
          </section>
        </div>
      )}
    </RumDetailPage>
  );
}

function title(session: rumApi.RumRow, t: ReturnType<typeof useTranslation>['t']): string {
  const headline = sessionHeadline(session);
  const kind =
    headline === 'error'
      ? t('session_detail.title_error')
      : headline === 'frustration'
        ? t('session_detail.title_frustration')
        : t('session_detail.title');

  return `${kind} · ${sessionPage(session) ?? '—'}`;
}

function RelatedTraces({
  rows,
  during,
}: {
  rows: RelatedTrace[];
  /** The stretch of time the session ran, for the links to its traces. */
  during: { from: string; to: string } | undefined;
}) {
  const { t } = useTranslation('rum');

  if (rows.length === 0) {
    return <div className="grid min-h-32 place-items-center text-sm text-tx-3">{t('session_detail.related_empty')}</div>;
  }

  return (
    <div className="grid gap-x-6 sm:grid-cols-2 xl:grid-cols-3">
      {rows.map((row) => (
        <div
          key={row.traceId}
          className="grid min-h-[72px] grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-bd-0 py-3"
        >
          <span className="min-w-0">
            <SignalReference type="trace_id" value={row.traceId} time={during}>
              {shortTraceId(row.traceId)}
            </SignalReference>
            <span className="mt-1 block truncate text-xs text-tx-3">
              {row.service ?? '—'} · {row.spanCount} {t('session_detail.related_columns.spans')}
            </span>
          </span>
          <span className="text-right">
            <span className="block font-mono text-xs font-strong text-tx-1">
              {row.durationMs != null ? `${Math.round(row.durationMs)} ms` : '—'}
            </span>
            <Pill tone={row.relation === 'direct' ? 'green' : 'yellow'}>
              {t(row.relation === 'direct' ? 'session_detail.relation_direct' : 'session_detail.relation_time_correlated')}
            </Pill>
          </span>
        </div>
      ))}
    </div>
  );
}

function shortTraceId(traceId: string): string {
  return traceId.length > 14 ? `${traceId.slice(0, 12)}…` : traceId;
}

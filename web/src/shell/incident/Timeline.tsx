import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as escalationsApi from '@/api/escalations';
import * as incidentsApi from '@/api/incidents';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type {
  Incident,
  IncidentTimeline,
  TimelineDelivery,
  TimelineEntry,
} from '@/types/alerting';

import { courseDuration } from './Course';
import { useImportance } from './Importance';
import { PipelineTraceRow } from './PipelineTrace';
import { refreshIncidents } from './refresh';
import { useTeams } from './Routing';
import { TimelineFilter, TimelinePhases, type TimelineView } from './TimelineHead';
import { categoryOfEntry, type TimelineCategory } from './timelineModel';

/** The longest note the backend keeps. */
const MAX_NOTE_CHARS = 4000;

interface Row {
  key: string;
  at: number;
  dot: string;
  category: TimelineCategory;
  body: React.ReactNode;
}

type Translate = ReturnType<typeof useTranslation<'alerts'>>['t'];

/** Which notification an event id belongs to: `alert.escalated:…` is an escalation, and so on. */
function noticeKind(eventId: string): 'triggered' | 'escalated' | 'acknowledged' | 'resolved' {
  const kind = eventId.split(':')[0]?.replace('alert.', '');
  return kind === 'escalated' || kind === 'acknowledged' || kind === 'resolved' ? kind : 'triggered';
}

/** One notification and everyone it reached, named with the connector it went through. */
function DeliveryLine({
  deliveries,
  nameOf,
  connectorName,
  t,
}: {
  deliveries: TimelineDelivery[];
  nameOf: (id: string) => string;
  connectorName: (id: string | null) => string | null;
  t: Translate;
}) {
  const first = deliveries[0];
  if (!first) return null;

  return (
    <span className="min-w-0">
      <span className="text-tx-1">{t(`incident_timeline.notice.${noticeKind(first.eventId)}`)}</span>
      <span className="text-tx-2">
        {' '}
        {deliveries
          .map((delivery) => {
            const who = delivery.userId ? nameOf(delivery.userId) : delivery.target ?? t('incident_timeline.someone');
            const through = connectorName(delivery.connectorId);
            // A policy an alert pipeline kept from sending tried nobody: it says why.
            if (delivery.status === 'skipped') {
              return t('incident_timeline.policy_skipped', { reason: delivery.error ?? '' });
            }

            const failed = delivery.status === 'failed';
            const via = through ? t('incident_timeline.recipient', { who, through }) : who;
            return failed ? t('incident_timeline.recipient_failed', { who: via }) : via;
          })
          .join(t('routing.list_separator'))}
      </span>
    </span>
  );
}

/** The words of one recorded step: who did what. */
function entryText(
  entry: TimelineEntry,
  names: {
    user: (id: string) => string;
    team: (id: string) => string;
    policy: (id: string) => string;
    urgency: (key: string) => string;
  },
  t: Translate,
): React.ReactNode {
  const actor = entry.actor ? names.user(entry.actor) : t('incident_timeline.system');
  const detail = entry.detail as Record<string, unknown>;
  const via = typeof detail.via === 'string' && detail.via !== 'web' ? t(`incident_timeline.via.${detail.via}`) : '';
  const list = (ids: unknown) =>
    Array.isArray(ids) ? ids.map((id) => names.user(String(id))).join(t('routing.list_separator')) : '';

  switch (entry.kind) {
    case 'acknowledged':
      return t('incident_timeline.acknowledged', { actor, via });
    case 'resolved':
      return t('incident_timeline.resolved', { actor, via });
    case 'escalated':
      return detail.reason === 'manual'
        ? t('incident_timeline.escalated_manual', { actor, n: Number(detail.step ?? 0) + 1 })
        : t('incident_timeline.escalated', { n: Number(detail.step ?? 0) + 1 });
    case 'reassigned': {
      const to = (detail.to ?? {}) as Record<string, unknown>;
      const target =
        to.kind === 'users'
          ? list(to.userIds)
          : to.kind === 'team'
            ? names.team(String(to.teamId))
            : names.policy(String(to.escalationPolicyId));
      return t('incident_timeline.reassigned', { actor, target });
    }
    case 'responders_added':
      return t('incident_timeline.responders_added', { actor, people: list(detail.userIds) });
    case 'snoozed':
      return t('incident_timeline.snoozed', { actor, until: formatMicrosActive(Number(detail.until)) });
    case 'snooze_cancelled':
      return t('incident_timeline.snooze_cancelled', { actor });
    case 'retriggered':
      return detail.reason === 'ack_timeout'
        ? t('incident_timeline.retriggered_ack_timeout')
        : t('incident_timeline.retriggered_snooze');
    case 'severity_changed':
      return t('incident_timeline.severity_changed', {
        actor,
        from: t(`history.severity.${String(detail.from)}`),
        to: t(`history.severity.${String(detail.to)}`),
      });
    case 'merged':
      return (
        <>
          {t('incident_timeline.merged', { actor })}{' '}
          <Link className="text-tx-0 underline-offset-2 hover:underline" to={`/alerts/incidents/${String(detail.into)}`}>
            {t('incident_timeline.merged_target')}
          </Link>
        </>
      );
    case 'merged_in':
      return t('incident_timeline.merged_in', {
        actor,
        count: Array.isArray(detail.incidents) ? detail.incidents.length : 0,
      });
    case 'merged_recovered':
      return t('incident_timeline.merged_recovered', { summary: String(detail.summary ?? '') });
    case 'remediation_requested':
      return t('incident_timeline.remediation_requested', { actor, action: String(detail.action ?? '') });
    case 'remediation_executed':
      return (
        <>
          {t(detail.status === 'failed' ? 'incident_timeline.remediation_failed' : 'incident_timeline.remediation_executed', {
            actor,
            action: String(detail.action ?? ''),
          })}
          {detail.output ? (
            <span className="mt-1 block break-all text-xs text-tx-2">{String(detail.output)}</span>
          ) : null}
        </>
      );
    case 'remediation_verified':
      return t(detail.passed ? 'incident_timeline.remediation_verified' : 'incident_timeline.remediation_unverified', {
        action: String(detail.action ?? ''),
      });
    case 'ticket_created':
      return (
        <>
          {t('incident_timeline.ticket_created', { actor, system: t(`tickets.systems.${String(detail.system)}`) })}{' '}
          <a className="text-tx-0 underline-offset-2 hover:underline" href={String(detail.url ?? '')} target="_blank" rel="noreferrer">
            {String(detail.key ?? '')}
          </a>
        </>
      );
    case 'ticket_updated':
      return t('incident_timeline.ticket_updated', { key: String(detail.key ?? ''), status: String(detail.status ?? '') });
    case 'declared_major':
      return detail.auto ? t('incident_timeline.declared_major_auto') : t('incident_timeline.declared_major', { actor });
    case 'role_assigned':
      return detail.userId
        ? t('incident_timeline.role_assigned', {
            actor,
            role: t(`major.roles.${String(detail.role)}`),
            person: names.user(String(detail.userId)),
          })
        : t('incident_timeline.role_cleared', { actor, role: t(`major.roles.${String(detail.role)}`) });
    case 'war_room_created':
      return (
        <>
          {t('incident_timeline.war_room_created')}{' '}
          {detail.url ? (
            <a className="text-tx-0 underline-offset-2 hover:underline" href={String(detail.url)} target="_blank" rel="noreferrer">
              {t('incident_timeline.join')}
            </a>
          ) : null}
        </>
      );
    case 'task_added':
      return t('incident_timeline.task_added', { actor, title: String(detail.title ?? '') });
    case 'task_done':
      return t('incident_timeline.task_done', { actor, title: String(detail.title ?? '') });
    case 'status_update':
      return (
        <>
          <span className="text-tx-1">{t('incident_timeline.status_update', { actor })}</span>
          <span className="mt-1 block whitespace-pre-wrap break-words rounded-md bg-bg-2 px-2.5 py-1.5 text-sm text-tx-0">
            {[detail.impact, detail.current, detail.next].filter(Boolean).map(String).join('\n')}
          </span>
        </>
      );
    case 'step_unreachable':
      return t('incident_timeline.step_unreachable', { n: Number(detail.step ?? 0) + 1 });
    case 'priority_changed':
      return t('incident_timeline.priority_changed', {
        actor,
        from: detail.from ? String(detail.from) : t('incident_timeline.no_priority'),
        to: detail.to ? String(detail.to) : t('incident_timeline.no_priority'),
      });
    case 'urgency_changed':
      return detail.reason === 'support_hours'
        ? t('incident_timeline.urgency_raised_support_hours', { to: names.urgency(String(detail.to)) })
        : t('incident_timeline.urgency_changed', {
            actor,
            from: names.urgency(String(detail.from)),
            to: names.urgency(String(detail.to)),
          });
    case 'pipeline_processed':
      return <PipelineTraceRow detail={entry.detail} />;
    case 'inhibited':
      return (
        <>
          {t('incident_timeline.inhibited', { rule: String(detail.ruleName ?? '') })}{' '}
          <Link
            className="text-tx-0 underline-offset-2 hover:underline"
            to={`/alerts/incidents/${String(detail.sourceIncidentId ?? '')}`}
          >
            {String(detail.sourceSummary ?? '')}
          </Link>
        </>
      );
    case 'inhibition_ended':
      return detail.reason === 'lifted'
        ? t('incident_timeline.inhibition_lifted', { actor })
        : detail.reason === 'rule_changed'
          ? t('incident_timeline.inhibition_rule_changed')
          : t('incident_timeline.inhibition_source_recovered');
    case 'note':
      return (
        <>
          <span className="text-tx-1">{t('incident_timeline.noted', { actor })}</span>
          <span className="mt-1 block whitespace-pre-wrap break-words rounded-md bg-bg-2 px-2.5 py-1.5 text-sm text-tx-0">
            {String(detail.text ?? '')}
          </span>
        </>
      );
  }
}

const DOT: Partial<Record<TimelineEntry['kind'], string>> = {
  inhibited: 'bg-indigo',
  acknowledged: 'bg-yellow',
  resolved: 'bg-green',
  escalated: 'bg-orange',
  retriggered: 'bg-orange',
  merged_recovered: 'bg-green',
  note: 'bg-indigo',
  remediation_executed: 'bg-indigo',
  remediation_verified: 'bg-green',
  ticket_created: 'bg-blue',
  declared_major: 'bg-red',
  status_update: 'bg-indigo',
  step_unreachable: 'bg-orange',
  pipeline_processed: 'bg-indigo',
};

/** Everything that happened, oldest first: the trigger, notifications, people's steps, notes and the end. */
function rowsOf(
  incident: Incident,
  timeline: IncidentTimeline | undefined,
  render: {
    entry: (entry: TimelineEntry) => React.ReactNode;
    deliveries: (deliveries: TimelineDelivery[]) => React.ReactNode;
  },
  t: Translate,
): Row[] {
  const rows: Row[] = [
    { key: 'fired', at: incident.createdAt, dot: 'bg-red', category: 'alert', body: t('drawer.timeline.created') },
  ];

  const byEvent = new Map<string, TimelineDelivery[]>();

  for (const delivery of timeline?.deliveries ?? []) {
    byEvent.set(delivery.eventId, [...(byEvent.get(delivery.eventId) ?? []), delivery]);
  }

  for (const [eventId, deliveries] of byEvent) {
    rows.push({
      key: `notice-${eventId}`,
      at: Math.min(...deliveries.map((delivery) => delivery.createdAt)),
      dot: 'bg-bd-2',
      category: 'notice',
      body: render.deliveries(deliveries),
    });
  }

  const entries = timeline?.entries ?? [];

  for (const entry of entries) {
    rows.push({
      key: entry.id,
      at: entry.at,
      dot: DOT[entry.kind] ?? 'bg-tx-3',
      category: categoryOfEntry(entry),
      body: render.entry(entry),
    });
  }

  // Recovered on its own (by its rule or source): nobody recorded it, the incident says when.
  if (incident.resolvedAt && !entries.some((entry) => entry.kind === 'resolved')) {
    rows.push({
      key: 'recovered',
      at: incident.resolvedAt,
      dot: 'bg-green',
      category: 'alert',
      body: t('incident_timeline.recovered'),
    });
  }

  return rows.sort((a, b) => a.at - b.at);
}

/**
 * What became of an incident: it fired, who was told and how it went, what people did with it, the
 * notes they left, and the incidents merged into it. Notes are written at the foot.
 */
export function IncidentTimelineView({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const users = useUsers();
  const teams = useTeams();
  const access = useActionAccess({ permission: 'alerts.incidents.update' });
  const [note, setNote] = React.useState('');
  const [view, setView] = React.useState<TimelineView>('all');

  const timeline = useQuery({
    queryKey: ['incident-timeline', incident.id],
    queryFn: () => incidentsApi.timeline(incident.id),
    staleTime: 10_000,
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
    staleTime: 60_000,
    retry: false,
  });
  const policies = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: () => escalationsApi.list(),
    staleTime: 60_000,
    retry: false,
  });

  const addNote = useMutation({
    mutationFn: () => incidentsApi.addNote(incident.id, note.trim()),
    onSuccess: async () => {
      setNote('');
      await refreshIncidents(queryClient, incident.id);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const levels = useImportance();

  const names = {
    user: users.nameOf,
    team: (id: string) => teams.data?.find((team) => team.id === id)?.name ?? id,
    policy: (id: string) => policies.data?.find((policy) => policy.id === id)?.name ?? id,
    // A level the organization named reads by its name; the built-in ones by their words.
    urgency: (key: string) =>
      levels.urgencies.find((level) => level.key === key)?.name || t(`urgency.${key}`, { defaultValue: key }),
  };
  const connectorName = (id: string | null) =>
    id ? connectors.data?.find((connector) => connector.id === id)?.name ?? null : null;

  const rows = rowsOf(
    incident,
    timeline.data,
    {
      entry: (entry) => entryText(entry, names, t),
      deliveries: (deliveries) => (
        <DeliveryLine deliveries={deliveries} nameOf={users.nameOf} connectorName={connectorName} t={t} />
      ),
    },
    t,
  );
  const merged = timeline.data?.merged ?? [];
  const tooLong = note.trim().length > MAX_NOTE_CHARS;
  const counts: Record<TimelineCategory, number> = { alert: 0, notice: 0, people: 0, notes: 0 };
  for (const row of rows) counts[row.category] += 1;
  const shownRows = view === 'all' ? rows : rows.filter((row) => row.category === view);

  return (
    <div className="flex flex-col gap-3">
      <TimelinePhases incident={incident} />
      <TimelineFilter counts={counts} value={view} onChange={setView} />
      <ol data-incident-timeline className="flex flex-col">
        {shownRows.map((row, index) => {
          const last = index === shownRows.length - 1;

          return (
            <li
              key={row.key}
              data-timeline-row
              className="grid grid-cols-[auto_minmax(0,1fr)_auto] gap-x-3 font-sans text-xs"
            >
              <span aria-hidden="true" className="flex flex-col items-center pt-[5px]">
                <span className={cn('h-2 w-2 shrink-0 rounded-full', row.dot)} />
                {!last && <span className="mt-1 w-px flex-1 bg-bd-1" />}
              </span>
              <span className={cn('min-w-0 text-tx-0', !last && 'pb-3')}>
                {row.body}
                {row.at > incident.createdAt && (
                  <span className="ml-2 tabular-nums text-tx-2">
                    {t('drawer.timeline.after', { duration: courseDuration(row.at - incident.createdAt) })}
                  </span>
                )}
              </span>
              <time
                dateTime={new Date(Math.floor(row.at / 1000)).toISOString()}
                className="shrink-0 tabular-nums text-tx-2"
              >
                {formatMicrosActive(row.at)}
              </time>
            </li>
          );
        })}
      </ol>

      {merged.length > 0 && (
        <div data-incident-merged className="flex flex-col gap-1">
          <span className="font-sans text-xs font-medium text-tx-2">
            {t('incident_timeline.merged_list', { count: merged.length })}
          </span>
          <ul className="flex flex-col gap-0.5">
            {merged.map((child) => (
              <li key={child.id} className="flex min-w-0 items-center gap-2 font-sans text-xs">
                <Link
                  to={`/alerts/incidents/${child.id}`}
                  className="min-w-0 truncate text-tx-0 underline-offset-2 hover:underline"
                >
                  {child.summary}
                </Link>
                <span className={child.status === 'resolved' ? 'text-[var(--green-ink)]' : 'text-tx-2'}>
                  {t(`center.incidents.status.${child.status}`)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {access.allowed && (
        <div data-incident-note className="flex flex-col gap-2">
          <FormTextarea
            rows={2}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder={t('incident_timeline.note_placeholder')}
            aria-label={t('incident_timeline.note_label')}
          />
          <div className="flex items-center justify-end gap-2">
            {tooLong && (
              <span className="font-sans text-xs text-[var(--red-ink)]">
                {t('incident_timeline.note_too_long', { max: MAX_NOTE_CHARS })}
              </span>
            )}
            <ChromeButton
              size="sm"
              disabled={!note.trim() || tooLong || addNote.isPending}
              onClick={() => addNote.mutate()}
              data-testid="incident-add-note"
            >
              {t('incident_timeline.add_note')}
            </ChromeButton>
          </div>
        </div>
      )}
    </div>
  );
}

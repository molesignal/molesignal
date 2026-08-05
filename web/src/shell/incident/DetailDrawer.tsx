import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellOff, Check, CircleCheck, Hash, Server, Sparkles, Tag, Workflow } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { MarkdownMessage } from '@/routes/intelligence/markdown';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { FormDrawer } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { LoadingState } from '@/shell/LoadingState';
import { SignalReference } from '@/shell/SignalReference';
import { toast } from '@/shell/ui/sonner';
import type { Incident, IncidentStatus, Severity } from '@/types/alerting';

import { IncidentSilenceDialog } from './SilenceDialog';

/**
 * Incident detail drawer — Phase 6 M1.1 #3 unblocked by the backend
 * schema extension (see BACKEND_REQUIREMENTS.md).
 *
 * Loads the full incident via `GET /alerts/incidents/{id}` (which
 * returns complete `trace_ids` / `host_ids` / `affected_services` /
 * `triggering_query.sample_values`, unlike the list endpoint which
 * truncates each handle list to the top 1 element).
 *
 * Cross-signal handles are wrapped in `SignalReference` so every
 * trace_id / host / service shows the HoverCard jump menu — direct
 * tech delivery of brief Principle #3 "Continuity across signals".
 */

export const SEVERITY_TONE: Record<Severity, PillTone> = {
  info: 'blue',
  warning: 'yellow',
  error: 'red',
  critical: 'red',
};

export const STATUS_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

interface IncidentDetailDrawerProps {
  /** Incident id to show. `null` closes the drawer. */
  incidentId: string | null;
  onClose: () => void;
}

export function IncidentDetailDrawer({ incidentId, onClose }: IncidentDetailDrawerProps) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const [silenceOpen, setSilenceOpen] = React.useState(false);
  const acknowledgePermission = useActionAccess({
    permission: 'alerts.acknowledge',
  });
  const silencePermission = useActionAccess({ permission: 'alerts.silence' });

  React.useEffect(() => {
    if (!incidentId) setSilenceOpen(false);
  }, [incidentId]);

  // `enabled: !!incidentId` keeps React Query idle when the drawer is closed.
  const q = useQuery({
    queryKey: ['incidents', incidentId],
    queryFn: () => incidentsApi.get(incidentId!),
    enabled: !!incidentId,
    staleTime: 10_000,
  });

  const ackMut = useMutation({
    mutationFn: (id: string) => incidentsApi.ack(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.ack_success', { defaultValue: 'Incident acknowledged' }));
      await queryClient.invalidateQueries({ queryKey: ['incidents', incidentId] });
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] });
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const resolveMut = useMutation({
    mutationFn: (id: string) => incidentsApi.resolve(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.resolve_success', { defaultValue: 'Incident resolved' }));
      await queryClient.invalidateQueries({ queryKey: ['incidents', incidentId] });
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] });
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const incident = q.data ?? null;
  const open = !!incidentId;
  const active = Boolean(
    incident &&
      (incident.status === 'open' || incident.status === 'acknowledged'),
  );
  const acknowledgeAccess = restrictActionAccess(
    acknowledgePermission,
    incident?.status === 'open',
    t('drawer.actions.ack_unavailable', {
      defaultValue: 'Only open incidents can be acknowledged.',
    }),
  );
  const resolveAccess = restrictActionAccess(
    acknowledgePermission,
    active,
    t('drawer.actions.resolve_unavailable', {
      defaultValue: 'This incident is no longer active.',
    }),
  );
  const silenceAccess = restrictActionAccess(
    silencePermission,
    active,
    t('drawer.actions.silence_unavailable', {
      defaultValue: 'Only active incidents can be silenced.',
    }),
  );

  return (
    <>
      <FormDrawer
        open={open}
        onOpenChange={(v) => {
          if (!v) onClose();
        }}
        title={incident?.summary ?? t('drawer.title_loading', { defaultValue: 'Incident' })}
        subtitle={
          incident ? (
            <span className="flex items-center gap-2">
              <Pill tone={SEVERITY_TONE[incident.severity]}>
                {t(`history.severity.${incident.severity}`)}
              </Pill>
              <Pill tone={STATUS_TONE[incident.status]}>
                {t(`center.incidents.status.${incident.status}`)}
              </Pill>
            </span>
          ) : undefined
        }
        footer={
          incident ? (
            <>
              <ChromeButton
                size="sm"
                disabled={silenceAccess.disabled}
                disabledReason={silenceAccess.reason}
                onClick={() => {
                  if (silenceAccess.allowed) setSilenceOpen(true);
                }}
                data-testid="incident-silence"
              >
                <BellOff className="h-3 w-3" />
                {t('drawer.actions.silence')}
              </ChromeButton>
              <ChromeButton
                size="sm"
                disabled={ackMut.isPending || acknowledgeAccess.disabled}
                disabledReason={!ackMut.isPending ? acknowledgeAccess.reason : undefined}
                onClick={() => {
                  if (acknowledgeAccess.allowed) ackMut.mutate(incident.id);
                }}
                data-testid="incident-ack"
              >
                <Check className="h-3 w-3" />
                {t('drawer.actions.ack', { defaultValue: 'Acknowledge' })}
              </ChromeButton>
              <ChromeButton
                size="sm"
                variant="primary"
                onClick={() => {
                  if (resolveAccess.allowed) resolveMut.mutate(incident.id);
                }}
                disabled={resolveMut.isPending || resolveAccess.disabled}
                disabledReason={!resolveMut.isPending ? resolveAccess.reason : undefined}
                data-testid="incident-resolve"
              >
                <CircleCheck className="h-3 w-3" />
                {t('drawer.actions.resolve', { defaultValue: 'Resolve' })}
              </ChromeButton>
            </>
          ) : null
        }
      >
        {q.isLoading && <LoadingState variant="list" rows={4} />}
        {q.isError && (
          <ErrorState
            error={q.error}
            title={t('drawer.error_title', { defaultValue: 'Cannot load incident' })}
            onRetry={() => void q.refetch()}
          />
        )}
        {incident && <IncidentBody incident={incident} />}
      </FormDrawer>
      <IncidentSilenceDialog
        incidentId={incident?.id ?? null}
        incidentName={incident?.summary}
        open={silenceOpen}
        onOpenChange={setSilenceOpen}
      />
    </>
  );
}

export function IncidentBody({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const timeWindow = React.useMemo(() => buildTimeWindow(incident), [incident]);

  return (
    <div className="flex flex-col gap-5">
      <Section
        title={t('drawer.incident_sections.timeline', { defaultValue: 'Timeline' })}
        icon={<Workflow className="h-3.5 w-3.5" />}
      >
        <Timeline incident={incident} />
      </Section>

      <IncidentRcaSection incidentId={incident.id} />

      {(incident.trace_ids.length > 0 ||
        incident.host_ids.length > 0 ||
        incident.affected_services.length > 0) && (
        <Section
          title={t('drawer.incident_sections.signals', { defaultValue: 'Cross-signal handles' })}
          icon={<Hash className="h-3.5 w-3.5" />}
        >
          {incident.affected_services.length > 0 && (
            <SignalRow
              label={t('drawer.signals.services', { defaultValue: 'Services' })}
              items={incident.affected_services}
              type="service"
              time={timeWindow}
            />
          )}
          {incident.trace_ids.length > 0 && (
            <SignalRow
              label={t('drawer.signals.traces', { defaultValue: 'Trace IDs' })}
              items={incident.trace_ids}
              type="trace_id"
              time={timeWindow}
            />
          )}
          {incident.host_ids.length > 0 && (
            <SignalRow
              label={t('drawer.signals.hosts', { defaultValue: 'Hosts' })}
              items={incident.host_ids}
              type="host"
              time={timeWindow}
            />
          )}
        </Section>
      )}

      {(Object.keys(incident.labels).length > 0 || Object.keys(incident.annotations).length > 0) && (
        <Section
          title={t('drawer.incident_sections.metadata', { defaultValue: 'Metadata' })}
          icon={<Tag className="h-3.5 w-3.5" />}
        >
          {Object.keys(incident.labels).length > 0 && (
            <KvBlock
              label={t('drawer.labels.title', { defaultValue: 'Labels' })}
              entries={incident.labels}
            />
          )}
          {Object.keys(incident.annotations).length > 0 && (
            <KvBlock
              label={t('drawer.annotations.title', { defaultValue: 'Annotations' })}
              entries={incident.annotations}
            />
          )}
        </Section>
      )}

      {incident.triggering_query && (
        <Section
          title={t('drawer.incident_sections.triggering_query', { defaultValue: 'Triggering query' })}
          icon={<Server className="h-3.5 w-3.5" />}
        >
          <TriggeringQueryBlock query={incident.triggering_query} />
        </Section>
      )}

      <Section
        title={t('drawer.incident_sections.assignees', { defaultValue: 'Assignees' })}
        icon={<CircleCheck className="h-3.5 w-3.5" />}
      >
        {incident.assignees.length === 0 ? (
          <p className="font-sans text-xs text-tx-3">
            {t('drawer.assignees_none', { defaultValue: 'No one assigned yet.' })}
          </p>
        ) : (
          <ul className="flex flex-wrap gap-1.5">
            {incident.assignees.map((a) => (
              <li key={a}>
                <Pill tone="indigo">{a}</Pill>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}

/* ─── AI root cause ──────────────────────────────────────────────── */

/**
 * AI root-cause analysis. The backend sweeper generates one for active
 * incidents within a couple of minutes; this also offers an on-demand
 * "Generate" button (`POST /alerts/incidents/{id}/rca`) so the user need
 * not wait. `getRca` returns `null` (not an error) until one exists.
 */
function IncidentRcaSection({ incidentId }: { incidentId: string }) {
  const { t, i18n } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const locale = incidentsApi.normalizeRcaLocale(i18n.resolvedLanguage ?? i18n.language);

  const q = useQuery({
    queryKey: ['incident-rca', incidentId],
    queryFn: () => incidentsApi.getRca(incidentId),
    enabled: !!incidentId,
    retry: false,
    staleTime: 30_000,
  });

  const genMut = useMutation({
    mutationFn: () => incidentsApi.generateRca(incidentId, locale),
    onSuccess: (rca) => {
      queryClient.setQueryData(['incident-rca', incidentId], rca);
      toast.success(t('drawer.rca.generated', { defaultValue: 'Root-cause analysis generated' }));
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const rca = q.data ?? null;
  const genLabel = genMut.isPending
    ? t('drawer.rca.generating', { defaultValue: 'Generating…' })
    : rca
      ? t('drawer.rca.regenerate', { defaultValue: 'Regenerate' })
      : t('drawer.rca.generate', { defaultValue: 'Generate' });

  const genButton = (
    <button
      type="button"
      onClick={() => genMut.mutate()}
      disabled={genMut.isPending}
      className={cn(
        'inline-flex h-8 items-center gap-1.5 font-sans text-xs font-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo disabled:opacity-50',
        rca
          ? 'rounded px-1 text-tx-2 hover:text-indigo-soft'
          : 'rounded-md border border-bd-1 bg-bg-2 px-3 text-tx-1 hover:bg-bg-3 hover:text-tx-0',
      )}
      data-testid="incident-rca-generate"
    >
      <Sparkles className="h-3 w-3" />
      {genLabel}
    </button>
  );

  return (
    <Section
      title={t('drawer.incident_sections.rca', { defaultValue: 'AI root cause' })}
      icon={<Sparkles className="h-3.5 w-3.5" />}
    >
      {q.isLoading ? (
        <LoadingState variant="list" rows={2} />
      ) : rca ? (
        <div className="flex flex-col gap-2">
          <div className="rounded border border-bd-0 bg-bg-2 px-3 py-2">
            <MarkdownMessage
              content={rca.summary}
              className="font-sans text-xs [&_h1]:text-sm [&_h2]:text-sm [&_h3]:text-xs [&_h4]:text-xs"
            />
          </div>
          <div className="flex items-center gap-2 font-sans text-xs text-tx-3">
            {rca.model && <span className="truncate">{rca.model}</span>}
            <span className="ml-auto tabular-nums">
              {new Date(rca.created_at / 1000).toLocaleString()}
            </span>
            {genButton}
          </div>
        </div>
      ) : (
        <div className="flex flex-col items-start gap-2">
          <p className="font-sans text-xs text-tx-3">
            {t('drawer.rca.empty', {
              defaultValue: 'No analysis yet — generate one or wait for the background pass.',
            })}
          </p>
          {genButton}
        </div>
      )}
    </Section>
  );
}

/* ─── Sub-blocks ─────────────────────────────────────────────────── */

function Section({
  title,
  icon,
  children,
}: {
  title: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="flex items-center gap-1.5 font-sans text-xs font-strong uppercase tracking-wider text-tx-3">
        {icon}
        {title}
      </h3>
      {children}
    </section>
  );
}

function SignalRow({
  label,
  items,
  type,
  time,
}: {
  label: string;
  items: string[];
  type: 'trace_id' | 'host' | 'service';
  time: { from: string; to: string };
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="font-sans text-xs text-tx-3">{label}</div>
      <ul className="flex flex-wrap gap-1.5">
        {items.map((v) => (
          <li key={v}>
            <SignalReference type={type} value={v} time={time}>
              {/* Truncate long IDs in the visible label; HoverCard still
                  shows the full value + copy button. */}
              {v.length > 24 ? `${v.slice(0, 12)}…${v.slice(-6)}` : v}
            </SignalReference>
          </li>
        ))}
      </ul>
    </div>
  );
}

function KvBlock({ label, entries }: { label: string; entries: Record<string, string> }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="font-sans text-xs text-tx-3">{label}</div>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded border border-bd-0 bg-bg-2 px-3 py-2 font-sans text-xs">
        {Object.entries(entries).map(([k, v]) => (
          <React.Fragment key={k}>
            <dt className="text-tx-3">{k}</dt>
            <dd className="break-all text-tx-1">{v}</dd>
          </React.Fragment>
        ))}
      </dl>
    </div>
  );
}

function TriggeringQueryBlock({ query }: { query: NonNullable<Incident['triggering_query']> }) {
  const { t } = useTranslation('alerts');
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5">
        <Pill tone="indigo">{query.language.toUpperCase()}</Pill>
        <span className="font-sans text-xs text-tx-3">
          {t('drawer.triggering_query.sample_count', {
            defaultValue: '{{count}} sample rows',
            count: query.sample_values.length,
          })}
        </span>
      </div>
      <pre className="overflow-auto rounded border border-bd-0 bg-bg-2 px-3 py-2 font-sans text-xs tabular-nums leading-relaxed text-tx-1">
        <code>{query.statement}</code>
      </pre>
      {query.sample_values.length > 0 && (
        <details className="rounded border border-bd-0 bg-bg-2">
          <summary
            className={cn(
              'cursor-pointer list-none px-3 py-1.5 font-sans text-xs font-strong text-tx-2',
              'hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-inset',
              '[&::-webkit-details-marker]:hidden',
            )}
          >
            {t('drawer.triggering_query.show_samples', { defaultValue: 'Show sample values' })}
          </summary>
          <ul className="border-t border-bd-0 px-3 py-2 font-sans text-xs tabular-nums">
            {query.sample_values.map((s, i) => (
              <li key={i} className="flex justify-between gap-3 border-b border-bd-0 py-1 last:border-b-0">
                <span className="text-tx-3">{new Date(s.ts / 1000).toLocaleString()}</span>
                <span className="truncate text-tx-2">{labelsToString(s.labels)}</span>
                <span className="font-strong text-tx-0">{s.value.toFixed(3)}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

function Timeline({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const events = [
    { ts: incident.created_at, label: t('drawer.timeline.created', { defaultValue: 'Created' }), tone: 'red' as const },
    incident.acknowledged_at
      ? {
          ts: incident.acknowledged_at,
          label: t('drawer.timeline.acknowledged', { defaultValue: 'Acknowledged' }) +
            (incident.acknowledged_by ? ` · ${incident.acknowledged_by}` : ''),
          tone: 'yellow' as const,
        }
      : null,
    incident.resolved_at
      ? {
          ts: incident.resolved_at,
          label:
            t('drawer.timeline.resolved', { defaultValue: 'Resolved' }) +
            (incident.resolved_by ? ` · ${incident.resolved_by}` : ''),
          tone: 'green' as const,
        }
      : null,
  ].filter(Boolean) as { ts: number; label: string; tone: 'red' | 'yellow' | 'green' }[];

  return (
    <ol className="flex flex-col gap-1.5">
      {events.map((e, i) => (
        <li key={i} className="flex items-center gap-2 font-sans text-xs">
          <span
            aria-hidden
            className={cn(
              'h-1.5 w-1.5 rounded-full',
              e.tone === 'red' && 'bg-red',
              e.tone === 'yellow' && 'bg-yellow',
              e.tone === 'green' && 'bg-green',
            )}
          />
          <span className="text-tx-1">{e.label}</span>
          <span className="ml-auto tabular-nums text-tx-3">{new Date(e.ts / 1000).toLocaleString()}</span>
        </li>
      ))}
    </ol>
  );
}

/* ─── helpers ────────────────────────────────────────────────────── */

/**
 * Build the time window used when jumping cross-signal: from incident
 * creation to now (or resolved_at if closed). Microsecond → ISO string.
 */
function buildTimeWindow(incident: Incident): { from: string; to: string } {
  const fromMs = Math.floor(incident.created_at / 1000);
  const toMs = incident.resolved_at ? Math.floor(incident.resolved_at / 1000) : Date.now();
  return {
    from: new Date(fromMs).toISOString(),
    to: new Date(toMs).toISOString(),
  };
}

function labelsToString(labels: Record<string, string>): string {
  return (
    Object.entries(labels)
      .map(([k, v]) => `${k}="${v}"`)
      .join(' · ') || '—'
  );
}

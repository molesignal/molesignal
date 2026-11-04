import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, CircleCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as alertSourcesApi from '@/api/alertSources';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton, Pill } from '@/shell/chrome';
import { IncidentActions } from '@/shell/incident/Actions';
import { IncidentBody } from '@/shell/incident/Body';
import { courseDuration } from '@/shell/incident/Course';
import { incidentSourceKey } from '@/shell/incident/ExternalAlert';
import { priorityLabel, urgencyNote, useImportance } from '@/shell/incident/Importance';
import { InvestigateWithAgent } from '@/shell/incident/InvestigateWithAgent';
import { externalSourceOf, hasRule, manualCreatorOf, syntheticCheckOf } from '@/shell/incident/origin';
import { refreshIncidents } from '@/shell/incident/refresh';
import { IncidentSilenceDialog } from '@/shell/incident/SilenceDialog';
import { STATUS_TONE } from '@/shell/incident/tones';
import { queryStateFor } from '@/shell/query/State';
import { STATEMENT_CELL, Statement } from '@/shell/readout/Statement';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { formatRelativeMicros } from '@/time/relative';
import type { Incident, Severity } from '@/types/alerting';

const SEVERITY_COLOR: Record<Severity, string> = {
  critical: 'var(--red)',
  error: 'var(--orange)',
  warning: 'var(--yellow)',
  info: 'var(--blue)',
};

const SEVERITY_TEXT: Record<Severity, string> = {
  critical: 'text-red',
  error: 'text-orange',
  warning: 'text-yellow',
  info: 'text-blue',
};

function isActive(incident: Incident): boolean {
  return incident.status === 'open' || incident.status === 'acknowledged';
}

/**
 * One incident on a page of its own: the address an incident is shared and
 * bookmarked by (`/alerts/incidents/:id`), and where the command palette and
 * a notification lead.
 *
 * It opens with how the incident stands — still firing and for how long, or
 * recovered and after how long — with who has it, when it fired and the rule
 * behind it beside that. Everything else is the same account the drawer
 * gives, laid out for the room a page has. It reads the incident under the
 * drawer's query key, so arriving from the drawer costs no request.
 */
export function IncidentDetail() {
  const { t, i18n } = useTranslation('alerts');
  const { id = '' } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const users = useUsers();
  const acknowledgePermission = useActionAccess({ permission: 'alerts.incidents.acknowledge' });
  const silencePermission = useActionAccess({ permission: 'alerts.incidents.silence' });
  const [silencing, setSilencing] = React.useState(false);

  const query = useQuery({
    queryKey: ['incidents', id],
    queryFn: () => incidentsApi.get(id),
    enabled: !!id,
    staleTime: 10_000,
  });

  const invalidate = () => refreshIncidents(queryClient, id);
  const acknowledge = useMutation({
    mutationFn: () => incidentsApi.ack(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.ack_success'));
      await invalidate();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const resolve = useMutation({
    mutationFn: () => incidentsApi.resolve(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.resolve_success'));
      await invalidate();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  // An incident that is gone, or another organization's, reads as "not found" and not as an
  // error. The last good copy is dropped with it, so the page does not keep showing an
  // incident the server no longer has.
  const notFound = query.isError && toApiError(query.error).status === 404;
  const incident = notFound ? null : query.data ?? null;
  // An incident a synthetic check or an alert source opened has no rule: its `rule_id` is theirs.
  const syntheticCheck = syntheticCheckOf(incident);
  const external = externalSourceOf(incident);
  const creator = manualCreatorOf(incident);
  const ruleQuery = useQuery({
    queryKey: ['alerts', 'rule', incident?.ruleId],
    queryFn: () => alertsApi.get(incident!.ruleId),
    enabled: hasRule(incident),
    staleTime: 30_000,
    retry: false,
  });
  const sourceQuery = useQuery({
    queryKey: incidentSourceKey(id),
    queryFn: () => alertSourcesApi.incidentSource(id),
    enabled: Boolean(external),
    staleTime: 15_000,
    retry: false,
  });
  const queryState = queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: incident });
  const pageState: ProductStateProps | null = notFound
    ? {
        variant: 'empty',
        title: t('incident_detail.not_found_title'),
        description: t('incident_detail.not_found_description'),
      }
    : productStateFor(queryState, { error: query.error });

  const active = incident ? isActive(incident) : false;
  const acknowledgeAccess = restrictActionAccess(
    acknowledgePermission,
    incident?.status === 'open' && !acknowledge.isPending,
    incident?.status === 'open' ? t('drawer.actions.pending') : t('drawer.actions.ack_unavailable'),
  );
  const resolveAccess = restrictActionAccess(
    acknowledgePermission,
    active && !resolve.isPending,
    active ? t('drawer.actions.pending') : t('drawer.actions.resolve_unavailable'),
  );
  const silenceAccess = restrictActionAccess(silencePermission, active, t('drawer.actions.silence_unavailable'));

  return (
    <>
      <DetailPage
        appearance="surface"
        title={incident?.summary ?? t('incident_detail.loading')}
        state={pageState}
        toolbar={
          // Only what still moves the incident forward: one that has recovered is a record,
          // and one somebody has taken is not offered to be taken again. Investigating it
          // is open to every incident.
          incident && (!active || incident.mergedInto) ? (
            <InvestigateWithAgent incident={incident} />
          ) : incident && active ? (
            <>
              <InvestigateWithAgent incident={incident} />
              <IncidentActions
                incident={incident}
                onSilence={silenceAccess.allowed ? () => setSilencing(true) : undefined}
              />
              {incident.status === 'open' && (
                <ChromeButton
                  onClick={() => acknowledgeAccess.allowed && acknowledge.mutate()}
                  disabled={acknowledgeAccess.disabled}
                  disabledReason={acknowledgeAccess.reason}
                  data-testid="incident-ack"
                >
                  <Check className="h-3.5 w-3.5" />
                  {t('drawer.actions.ack')}
                </ChromeButton>
              )}
              <ChromeButton
                variant="primary"
                onClick={() => resolveAccess.allowed && resolve.mutate()}
                disabled={resolveAccess.disabled}
                disabledReason={resolveAccess.reason}
                data-testid="incident-resolve"
              >
                <CircleCheck className="h-3.5 w-3.5" />
                {t('drawer.actions.resolve')}
              </ChromeButton>
            </>
          ) : undefined
        }
      >
        {incident && (
          <>
            <IncidentStanding
              incident={incident}
              ruleName={ruleQuery.data?.name}
              syntheticCheck={syntheticCheck}
              externalSource={
                external
                  ? {
                      id: external.id,
                      kind: external.kind,
                      name: sourceQuery.isSuccess ? (sourceQuery.data.source?.name ?? null) : undefined,
                    }
                  : null
              }
              creatorName={creator ? users.nameOf(creator.userId) : null}
              ownerName={
                incident.acknowledgedBy || incident.assignees[0]
                  ? users.nameOf(incident.acknowledgedBy ?? incident.assignees[0] ?? '')
                  : undefined
              }
              language={i18n.language}
            />
            <IncidentBody incident={incident} rule={ruleQuery.data} layout="page" />
          </>
        )}
      </DetailPage>
      <IncidentSilenceDialog
        incidentId={incident?.id ?? null}
        incidentName={incident?.summary}
        open={silencing}
        onOpenChange={setSilencing}
      />
    </>
  );
}

function IncidentStanding({
  incident,
  ruleName,
  syntheticCheck,
  externalSource,
  creatorName,
  ownerName,
  language,
}: {
  incident: Incident;
  ruleName: string | undefined;
  /** Who opened the incident by hand, in place of a rule. */
  creatorName: string | null;
  /** The check that opened the incident, in place of a rule. */
  syntheticCheck: string | null;
  /** The alert source that opened the incident, in place of a rule: its name is `null` once it was deleted. */
  externalSource: { id: string; kind: string; name: string | null | undefined } | null;
  ownerName: string | undefined;
  language: string;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const active = isActive(incident);
  const nowMicros = Date.now() * 1000;
  const service = incident.affectedServices[0] ?? incident.labels.service ?? incident.labels.svc;
  const lasted = courseDuration((incident.resolvedAt ?? nowMicros) - incident.createdAt);

  return (
    <Statement
      label={t('incident_detail.loading')}
      tone={active ? SEVERITY_COLOR[incident.severity] : 'var(--green)'}
      eyebrow={
        <span data-testid="incident-severity">
          {[
            t(`severity.${incident.severity}`),
            incident.priority ? priorityLabel(levels, incident.priority) : null,
            urgencyNote(levels, incident, t),
            service,
          ]
            .filter(Boolean)
            .join(' · ')}
        </span>
      }
      title={
        active
          ? t('incident_detail.firing_for', { duration: lasted })
          : t('incident_detail.recovered_after', { duration: lasted })
      }
      titleClassName={active ? SEVERITY_TEXT[incident.severity] : 'text-green'}
      dataAttributes={{ 'data-incident-standing': incident.status }}
      readouts={
        <>
          <Fact label={t('incident_detail.meta.status')}>
            <span data-testid="incident-status">
              <Pill tone={STATUS_TONE[incident.status]}>{t(`center.incidents.status.${incident.status}`)}</Pill>
            </span>
            <span className="mt-1.5 block truncate text-xs font-normal text-tx-2">
              {ownerName ?? t('incident_detail.no_owner')}
            </span>
          </Fact>
          <Fact label={t('incident_detail.meta.fired')}>
            <span className="tabular-nums">{formatMicrosActive(incident.createdAt)}</span>
            <span className="mt-1.5 block truncate text-xs font-normal text-tx-2">
              {formatRelativeMicros(incident.createdAt, language, nowMicros)}
            </span>
          </Fact>
          <Fact
            label={t(
              syntheticCheck
                ? 'incident_detail.meta.synthetic'
                : externalSource
                  ? 'incident_detail.meta.source'
                  : creatorName !== null
                    ? 'incident_detail.meta.created_by'
                    : 'incident_detail.meta.rule',
            )}
          >
            {creatorName !== null ? (
              <span className="block truncate" data-incident-created-by>
                {creatorName || t('incident_detail.someone')}
              </span>
            ) : externalSource ? (
              externalSource.name === null ? (
                <span className="text-tx-2">{t('alert_sources.incident.deleted')}</span>
              ) : (
                <Link
                  to={`/alerts/sources/${encodeURIComponent(externalSource.id)}`}
                  className="block truncate underline-offset-4 outline-none hover:underline focus-visible:underline"
                >
                  {externalSource.name ?? t(`alert_sources.kinds.${externalSource.kind}`, { defaultValue: externalSource.kind })}
                </Link>
              )
            ) : syntheticCheck ? (
              <Link
                to={`/synthetics/checks/${encodeURIComponent(syntheticCheck)}`}
                className="block truncate underline-offset-4 outline-none hover:underline focus-visible:underline"
              >
                {incident.summary}
              </Link>
            ) : ruleName ? (
              <Link
                to={`/alerts/rules/${encodeURIComponent(incident.ruleId)}/edit`}
                className="block truncate underline-offset-4 outline-none hover:underline focus-visible:underline"
              >
                {ruleName}
              </Link>
            ) : (
              <span className="text-tx-2">{t('insights.rules.unknown_rule')}</span>
            )}
            <span className="mt-1.5 block truncate font-mono text-xs font-normal text-tx-2">
              {incident.fingerprint}
            </span>
          </Fact>
        </>
      }
    />
  );
}

/** A fact beside the verdict that is a name or a moment, not a number to count up to. */
function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className={`ms-rise min-w-0 ${STATEMENT_CELL}`}>
      <div className="type-label truncate font-medium text-tx-2">{label}</div>
      <div className="mt-1.5 min-w-0 font-sans text-md font-strong text-tx-0">{children}</div>
    </div>
  );
}

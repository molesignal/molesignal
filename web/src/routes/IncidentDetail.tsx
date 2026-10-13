import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellOff, Check, CircleCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton, Pill } from '@/shell/chrome';
import { IncidentBody } from '@/shell/incident/Body';
import { courseDuration } from '@/shell/incident/Course';
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
  const acknowledgePermission = useActionAccess({ permission: 'alerts.acknowledge' });
  const silencePermission = useActionAccess({ permission: 'alerts.silence' });
  const [silencing, setSilencing] = React.useState(false);

  const query = useQuery({
    queryKey: ['incidents', id],
    queryFn: () => incidentsApi.get(id),
    enabled: !!id,
    staleTime: 10_000,
  });

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ['incidents', id] });
    await queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] });
  };
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
  const ruleQuery = useQuery({
    queryKey: ['alerts', 'rule', incident?.rule_id],
    queryFn: () => alertsApi.get(incident!.rule_id),
    enabled: Boolean(incident?.rule_id),
    staleTime: 30_000,
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
          // and one somebody has taken is not offered to be taken again.
          incident && active ? (
            <>
              <ChromeButton
                onClick={() => silenceAccess.allowed && setSilencing(true)}
                disabled={silenceAccess.disabled}
                disabledReason={silenceAccess.reason}
                data-testid="incident-silence"
              >
                <BellOff className="h-3.5 w-3.5" />
                {t('drawer.actions.silence')}
              </ChromeButton>
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
              ownerName={
                incident.acknowledged_by || incident.assignees[0]
                  ? users.nameOf(incident.acknowledged_by ?? incident.assignees[0] ?? '')
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
  ownerName,
  language,
}: {
  incident: Incident;
  ruleName: string | undefined;
  ownerName: string | undefined;
  language: string;
}) {
  const { t } = useTranslation('alerts');
  const active = isActive(incident);
  const nowMicros = Date.now() * 1000;
  const service = incident.affected_services[0] ?? incident.labels.service ?? incident.labels.svc;
  const lasted = courseDuration((incident.resolved_at ?? nowMicros) - incident.created_at);

  return (
    <Statement
      label={t('incident_detail.loading')}
      tone={active ? SEVERITY_COLOR[incident.severity] : 'var(--green)'}
      eyebrow={
        <span data-testid="incident-severity">
          {[t(`severity.${incident.severity}`), service].filter(Boolean).join(' · ')}
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
            <span className="tabular-nums">{formatMicrosActive(incident.created_at)}</span>
            <span className="mt-1.5 block truncate text-xs font-normal text-tx-2">
              {formatRelativeMicros(incident.created_at, language, nowMicros)}
            </span>
          </Fact>
          <Fact label={t('incident_detail.meta.rule')}>
            {ruleName ? (
              <Link
                to={`/alerts/rules/${encodeURIComponent(incident.rule_id)}/edit`}
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

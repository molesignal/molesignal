import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { BellOff, Check, CircleCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { FormDrawer } from '@/shell/FormDrawer';
import { LoadingState } from '@/shell/LoadingState';
import { toast } from '@/shell/ui/sonner';

import { IncidentBody } from './Body';
import { InvestigateWithAgent } from './InvestigateWithAgent';
import { IncidentSilenceDialog } from './SilenceDialog';
import { SEVERITY_TONE, STATUS_TONE } from './tones';

/**
 * Incident detail drawer. Loads the full incident via
 * `GET /alerts/incidents/{id}` (which
 * returns complete `trace_ids` / `host_ids` / `affected_services` /
 * `triggering_query.sample_values`, unlike the list endpoint which
 * truncates each handle list to the top 1 element).
 *
 * Cross-signal handles use `SignalReference` so every trace_id, host, and
 * service exposes the shared HoverCard jump menu.
 */

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
  const ruleQuery = useQuery({
    queryKey: ['alerts', 'rule', incident?.rule_id],
    queryFn: () => alertsApi.get(incident!.rule_id),
    enabled: Boolean(incident?.rule_id),
    staleTime: 30_000,
  });
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
              <span className="mr-auto">
                <InvestigateWithAgent incident={incident} size="sm" />
              </span>
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
        {incident && (
          <IncidentBody incident={incident} rule={ruleQuery.data} />
        )}
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

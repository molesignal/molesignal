import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, CircleCheck } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';

import { type MetadataStripItem } from '@/admin';
import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor, type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton, Pill } from '@/shell/chrome';
import { IncidentBody, SEVERITY_TONE, STATUS_TONE } from '@/shell/incident/DetailDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

/**
 * Incident detail — full-page sibling of {@link IncidentDetailDrawer}.
 *
 * The Alerts table opens incidents in a drawer; this page backs the
 * deep-link / keyboard-nav route `/alerts/incidents/:id` (and the
 * command palette) so an incident is shareable and bookmarkable. It
 * reuses the drawer's {@link IncidentBody} renderer verbatim — one
 * source of truth for timeline / cross-signal handles / triggering
 * query — and shares the `['incidents', id]` query key so navigating
 * from the drawer is a cache hit.
 */
export function IncidentDetail() {
  const { t } = useTranslation('alerts');
  const { id = '' } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const acknowledgeAccess = useActionAccess({
    permission: 'alerts.acknowledge',
  });

  const q = useQuery({
    queryKey: ['incidents', id],
    queryFn: () => incidentsApi.get(id),
    enabled: !!id,
    staleTime: 10_000,
  });

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey: ['incidents', id] });
    await queryClient.invalidateQueries({ queryKey: ['alerts', 'incidents'] });
  };

  const ackMut = useMutation({
    mutationFn: () => incidentsApi.ack(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.ack_success', { defaultValue: 'Incident acknowledged' }));
      await invalidate();
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  const resolveMut = useMutation({
    mutationFn: () => incidentsApi.resolve(id),
    onSuccess: async () => {
      toast.success(t('drawer.actions.resolve_success', { defaultValue: 'Incident resolved' }));
      await invalidate();
    },
    onError: (err: unknown) => toast.error(toApiError(err).message),
  });

  // 404 (deleted / cross-org) reads as a clean "not found" empty rather
  // than a scary error banner; any other failure stays an error. On 404
  // we also drop any stale cached incident so the body, metadata strip,
  // and toolbar all agree on "not found" — react-query keeps the last
  // good `data` across a failed background refetch otherwise.
  const notFound = q.isError && toApiError(q.error).status === 404;
  const incident = notFound ? null : q.data ?? null;
  const queryState = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: incident });
  const pageState: ProductStateProps | null = notFound
    ? {
        variant: 'empty',
        title: t('incident_detail.not_found_title', { defaultValue: 'Incident not found' }),
        description: t('incident_detail.not_found_description', {
          defaultValue: 'This incident may have been purged or belongs to another organization.',
        }),
      }
    : productStateFor(queryState, { error: q.error });

  const canAck = incident?.status === 'open';
  const canResolve = incident?.status === 'open' || incident?.status === 'acknowledged';
  const ackAccess = restrictActionAccess(
    acknowledgeAccess,
    Boolean(canAck) && !ackMut.isPending,
    canAck
      ? t('drawer.actions.pending', {
          defaultValue: 'Another incident update is in progress.',
        })
      : t('drawer.actions.ack_unavailable', {
          defaultValue: 'Only open incidents can be acknowledged.',
        }),
  );
  const resolveAccess = restrictActionAccess(
    acknowledgeAccess,
    Boolean(canResolve) && !resolveMut.isPending,
    canResolve
      ? t('drawer.actions.pending', {
          defaultValue: 'Another incident update is in progress.',
        })
      : t('drawer.actions.resolve_unavailable', {
          defaultValue: 'This incident is already resolved.',
        }),
  );

  const metadata: MetadataStripItem[] | undefined = incident
    ? [
        {
          label: t('incident_detail.meta.status', { defaultValue: 'Status' }),
          value: (
            <span data-testid="incident-status">
              <Pill tone={STATUS_TONE[incident.status]}>{incident.status}</Pill>
            </span>
          ),
        },
        {
          label: t('incident_detail.meta.severity', { defaultValue: 'Severity' }),
          value: (
            <span data-testid="incident-severity">
              <Pill tone={SEVERITY_TONE[incident.severity]}>{incident.severity}</Pill>
            </span>
          ),
        },
        {
          label: t('incident_detail.meta.created', { defaultValue: 'Created' }),
          value: <span className="tabular-nums">{formatMicrosActive(incident.created_at)}</span>,
        },
        {
          label: t('incident_detail.meta.fingerprint', { defaultValue: 'Fingerprint' }),
          value: <span className="break-all tabular-nums">{incident.fingerprint}</span>,
        },
      ]
    : undefined;

  return (
    <DetailPage
      title={incident?.summary ?? t('incident_detail.loading', { defaultValue: 'Incident' })}
      metadata={metadata}
      state={pageState}
      toolbar={
        incident ? (
          <>
            <ChromeButton
              onClick={() => ackAccess.allowed && ackMut.mutate()}
              disabled={ackAccess.disabled}
              disabledReason={ackAccess.reason}
              data-testid="incident-ack"
            >
              <Check className="h-3 w-3" />
              {t('drawer.actions.ack', { defaultValue: 'Acknowledge' })}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              onClick={() => resolveAccess.allowed && resolveMut.mutate()}
              disabled={resolveAccess.disabled}
              disabledReason={resolveAccess.reason}
              data-testid="incident-resolve"
            >
              <CircleCheck className="h-3 w-3" />
              {t('drawer.actions.resolve', { defaultValue: 'Resolve' })}
            </ChromeButton>
          </>
        ) : undefined
      }
    >
      {incident && <IncidentBody incident={incident} />}
    </DetailPage>
  );
}

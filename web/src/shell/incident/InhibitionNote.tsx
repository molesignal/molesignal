import { useMutation, useQueryClient } from '@tanstack/react-query';
import { BellRing } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import type { Incident } from '@/types/alerting';

import { refreshIncidents } from './refresh';

/**
 * Why nobody was paged about this incident: another one holds it back, or it is waiting to see
 * whether one comes. A person may disagree, so the way to notify anyway sits beside the reason.
 */
export function InhibitionNote({ incident, nowMicros = Date.now() * 1000 }: { incident: Incident; nowMicros?: number }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.incidents.update' });
  const active = incident.status === 'open' || incident.status === 'acknowledged';
  const held = incident.inhibition;
  const waiting = !held && (incident.inhibitionWaitUntil ?? 0) > nowMicros;

  const lift = useMutation({
    mutationFn: () => incidentsApi.liftInhibition(incident.id),
    onSuccess: () => {
      toast.success(t('inhibitions.note.lifted'));
      void refreshIncidents(queryClient, incident.id);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (!active || (!held && !waiting)) return null;

  return (
    <div
      data-incident-inhibition={held ? 'held' : 'waiting'}
      className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-[var(--control-surface)] px-3 py-2 font-sans text-xs text-tx-1"
    >
      <span className="min-w-0 flex-1">
        {held ? (
          <>
            {t('inhibitions.note.held_by')}{' '}
            <Link
              className="text-tx-0 underline-offset-2 hover:underline"
              to={`/alerts/incidents/${held.sourceIncidentId}`}
            >
              {held.sourceSummary}
            </Link>{' '}
            {t('inhibitions.note.rule', { rule: held.ruleName })}
            {held.endingAt ? ` ${t('inhibitions.note.ending', { time: formatMicrosActive(held.endingAt, false) })}` : ''}
          </>
        ) : (
          t('inhibitions.note.waiting', { time: formatMicrosActive(incident.inhibitionWaitUntil ?? 0, false) })
        )}
      </span>
      <ChromeButton
        size="sm"
        disabled={access.disabled || lift.isPending}
        disabledReason={access.reason}
        onClick={() => lift.mutate()}
        data-incident-lift-inhibition
      >
        <BellRing className="h-3 w-3" />
        {t('inhibitions.note.lift')}
      </ChromeButton>
    </div>
  );
}

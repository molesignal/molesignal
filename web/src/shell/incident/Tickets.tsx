import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Ticket } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as ticketsApi from '@/api/tickets';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';
import type { Incident } from '@/types/alerting';

import { IncidentSection } from './Section';

/** The tickets made for the incident in Jira, ServiceNow or Meegle, and making one. */
export function IncidentTickets({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const createAccess = useActionAccess({ permission: 'alerts.incidents.tickets' });
  const key = ['incident-tickets', incident.id];
  const tickets = useQuery({ queryKey: key, queryFn: () => ticketsApi.incidentTickets(incident.id), retry: false });
  const connectors = useQuery({ queryKey: ['ticket-connectors'], queryFn: ticketsApi.listConnectors, retry: false, staleTime: 60_000 });
  const used = new Set((tickets.data ?? []).map((ticket) => ticket.connectorId));
  const free = (connectors.data ?? []).filter((connector) => connector.enabled && !used.has(connector.id));
  const [connectorId, setConnectorId] = React.useState('');
  const chosen = free.find((connector) => connector.id === connectorId) ?? free[0];
  const create = useMutation({
    mutationFn: (id: string) => ticketsApi.createTicket(incident.id, id),
    onSuccess: (ticket) => {
      toast.success(t('tickets.incident.created', { key: ticket.externalKey }));
      void queryClient.invalidateQueries({ queryKey: key });
      void queryClient.invalidateQueries({ queryKey: ['incident-timeline', incident.id] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const list = tickets.data ?? [];
  const systemOf = (id: string) => connectors.data?.find((connector) => connector.id === id)?.name;

  if (list.length === 0 && (connectors.data ?? []).length === 0) return null;

  return (
    <IncidentSection title={t('drawer.incident_sections.tickets')} icon={<Ticket className="h-3.5 w-3.5" />} surface={surface}>
      <div data-incident-tickets className="flex flex-col gap-3 font-sans text-xs">
        {list.length === 0 ? (
          <p className="text-tx-2">{t('tickets.incident.none')}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {list.map((ticket) => (
              <li key={ticket.id} data-incident-ticket={ticket.externalKey} className="min-w-0">
                <div className="flex min-w-0 items-baseline gap-2">
                  <span className="shrink-0 text-tx-2">{t(`tickets.systems.${ticket.system}`)}</span>
                  <a
                    href={ticket.url}
                    target="_blank"
                    rel="noreferrer"
                    className="min-w-0 truncate text-sm text-tx-0 underline-offset-2 hover:underline"
                  >
                    {ticket.externalKey}
                  </a>
                  {ticket.status && (
                    <span className={cn('shrink-0 font-strong', ticket.done ? 'text-green' : 'text-blue')}>{ticket.status}</span>
                  )}
                </div>
                <p className="mt-0.5 text-tx-2">
                  {[systemOf(ticket.connectorId), t('tickets.incident.synced', { value: formatMicrosActive(ticket.syncedAt, false) })]
                    .filter(Boolean)
                    .join(' · ')}
                </p>
                {ticket.lastError && <p className="mt-0.5 truncate text-red">{ticket.lastError}</p>}
              </li>
            ))}
          </ul>
        )}
        {chosen && (
          <div className="flex min-w-0 items-center gap-2">
            {free.length > 1 && (
              <div className="min-w-0 flex-1">
                <FormSelect
                  value={chosen.id}
                  onChange={setConnectorId}
                  options={free.map((connector) => ({ value: connector.id, label: connector.name }))}
                  ariaLabel={t('tickets.incident.connector')}
                />
              </div>
            )}
            <ChromeButton
              size="sm"
              disabled={create.isPending || createAccess.disabled}
              disabledReason={createAccess.reason}
              onClick={() => create.mutate(chosen.id)}
            >
              {free.length > 1 ? t('tickets.incident.create') : t('tickets.incident.create_in', { name: chosen.name })}
            </ChromeButton>
          </div>
        )}
      </div>
    </IncidentSection>
  );
}

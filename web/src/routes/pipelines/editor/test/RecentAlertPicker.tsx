import { useQuery } from '@tanstack/react-query';
import { LoaderCircle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { FormSelect } from '@/shell/FormDrawer';

import { alertEventOf, recentIncidents } from './recentEvent';

/**
 * Picks one of the latest incidents as the sample of an alert pipeline, as the
 * steps would see its event. Anyone who cannot read incidents is offered none.
 */
export function RecentAlertPicker({
  onPick,
  onFailure,
}: {
  onPick: (event: Record<string, unknown>, metadata: Record<string, string>) => void;
  onFailure: (reason: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const access = useActionAccess({ permission: 'alerts.incidents.read' });
  const [loading, setLoading] = React.useState(false);
  const [picked, setPicked] = React.useState('');

  const incidents = useQuery({
    queryKey: ['pipeline-test', 'recent-incidents'],
    queryFn: recentIncidents,
    enabled: access.allowed,
    staleTime: 30_000,
    retry: false,
  });

  if (!access.allowed || incidents.isError) return null;

  // The list holds only the first of an incident's services: the sample takes it whole.
  const pick = async (id: string) => {
    setPicked(id);
    setLoading(true);
    try {
      const { event, metadata } = alertEventOf(await incidentsApi.get(id));
      onPick(event, metadata);
    } catch (error) {
      onFailure(toApiError(error).message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div data-recent-alert className="flex w-[260px] min-w-0 items-center gap-[6px]">
      {loading && <LoaderCircle aria-hidden className="h-3 w-3 shrink-0 animate-spin text-tx-3" />}
      <div className="min-w-0 flex-1">
        <FormSelect
          value={picked}
          onChange={(id) => void pick(id)}
          ariaLabel={t('test.recent_alert_label')}
          placeholder={t(
            incidents.data?.length === 0 ? 'test.no_recent_alert' : 'test.use_recent_alert',
          )}
          disabled={!incidents.data?.length}
          options={(incidents.data ?? []).map((incident) => ({
            value: incident.id,
            label: `${incident.summary} · ${formatMicrosActive(incident.createdAt)}`,
          }))}
        />
      </div>
    </div>
  );
}

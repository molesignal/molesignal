import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as alertsApi from '@/api/alerts';
import * as incidentsApi from '@/api/incidents';
import type { FrameProps } from '@/investigation/frame';
import { Pill } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { IncidentBody } from '@/shell/incident/Body';
import { hasRule } from '@/shell/incident/origin';
import { SEVERITY_TONE, STATUS_TONE } from '@/shell/incident/tones';
import { LoadingState } from '@/shell/LoadingState';

/**
 * An incident on the investigation stack: the same body as its drawer
 * (signals, root-cause analysis with its evidence, timeline), read from
 * `GET /alerts/incidents/{id}`.
 */
export function IncidentFrame({ frame }: FrameProps) {
  const { t } = useTranslation('alerts');
  const id = typeof frame.params.id === 'string' ? frame.params.id : '';

  const query = useQuery({
    queryKey: ['incidents', id],
    queryFn: () => incidentsApi.get(id),
    enabled: Boolean(id),
    staleTime: 10_000,
  });
  const incident = query.data;
  const rule = useQuery({
    queryKey: ['alerts', 'rule', incident?.ruleId],
    queryFn: () => alertsApi.get(incident!.ruleId),
    enabled: hasRule(incident),
    staleTime: 30_000,
  });

  if (query.isLoading) return <LoadingState variant="list" rows={4} />;
  if (query.isError || !incident) {
    return <ErrorState error={query.error} title={t('drawer.error_title', { defaultValue: 'Cannot load incident' })} onRetry={() => void query.refetch()} />;
  }

  return (
    <div data-incident-frame={incident.id} className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div className="flex flex-col gap-1.5">
        <h2 className="font-sans text-sm font-strong text-tx-0">{incident.summary}</h2>
        <span className="flex items-center gap-2">
          <Pill tone={SEVERITY_TONE[incident.severity]}>{t(`history.severity.${incident.severity}`)}</Pill>
          <Pill tone={STATUS_TONE[incident.status]}>{t(`center.incidents.status.${incident.status}`)}</Pill>
        </span>
      </div>
      <IncidentBody incident={incident} rule={rule.data} />
    </div>
  );
}

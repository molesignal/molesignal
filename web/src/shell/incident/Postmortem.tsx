import { useQuery } from '@tanstack/react-query';
import { NotebookPen } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as postmortemsApi from '@/api/postmortems';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import type { Incident } from '@/types/alerting';

import { PostmortemDrawer, useStartPostmortem } from './PostmortemDrawer';
import { IncidentSection } from './Section';

/** The incident's internal postmortem: writing one, or opening the one there is. */
export function IncidentPostmortem({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const readAccess = useActionAccess({ permission: 'postmortems.read' });
  const manageAccess = useActionAccess({ permission: 'postmortems.manage' });
  const [open, setOpen] = React.useState(false);
  const detail = useQuery({
    queryKey: ['incident-postmortem', incident.id],
    queryFn: () => postmortemsApi.forIncident(incident.id),
    enabled: readAccess.allowed,
    retry: false,
  });
  const start = useStartPostmortem(incident);
  const postmortem = detail.data?.postmortem ?? null;

  if (!readAccess.allowed) return null;

  return (
    <IncidentSection title={t('drawer.incident_sections.postmortem')} icon={<NotebookPen className="h-3.5 w-3.5" />} surface={surface}>
      <div data-incident-postmortem className="flex items-center gap-3 font-sans text-xs">
        <p className="min-w-0 flex-1 text-tx-2">
          {postmortem
            ? t('postmortems.summary_line', {
                status: t(`postmortems.status.${postmortem.status}`),
                count: (detail.data?.actions ?? []).filter((item) => item.status === 'open').length,
              })
            : t('postmortems.none')}
        </p>
        {postmortem ? (
          <ChromeButton size="sm" onClick={() => setOpen(true)}>
            {t('postmortems.open')}
          </ChromeButton>
        ) : (
          <ChromeButton
            size="sm"
            disabled={start.isPending || manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => start.mutate(undefined, { onSuccess: () => setOpen(true) })}
          >
            {t('postmortems.start')}
          </ChromeButton>
        )}
      </div>
      <PostmortemDrawer open={open} incidentId={incident.id} incidentSummary={incident.summary} onClose={() => setOpen(false)} />
    </IncidentSection>
  );
}

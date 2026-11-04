import { BellRing } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumApplication } from '@/api/rum';
import { useActionAccess } from '@/product/actionAccess';
import { IncidentDetailDrawer } from '@/shell/incident/DetailDrawer';
import { cn } from '@/shell/lib/cn';

import { AlertsDrawer } from './AlertsDrawer';
import { incidentsOf, SEVERITY_TEXT } from './model';
import { useActiveIncidents } from './useRumAlerts';

/**
 * Alerts, at the end of the row that says what a page is scoped to: the number of incidents going on
 * for the application, in the colour of the worst, and the drawer of its alerts behind it. Those who
 * may not read alerts do not see it. An incident opens over the page, in the drawer of the incidents.
 */
export function RumAlertsButton({
  application,
  environment,
  className,
}: {
  application: RumApplication;
  environment: string;
  /** The look of a control of the row. */
  className?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const reader = useActionAccess({ permission: 'alerts.rules.read' });
  const incidents = useActiveIncidents(reader.allowed);
  const [open, setOpen] = React.useState(false);
  const [viewing, setViewing] = React.useState<string | null>(null);

  if (!reader.allowed) return null;

  const going = incidentsOf(incidents.data, { application: application.id, environment });
  const worst = going[0]?.severity;

  return (
    <>
      <button
        type="button"
        data-rum-alerts-button=""
        data-rum-alerts-count={going.length}
        aria-label={going.length > 0 ? t('alerts.button_aria', { count: going.length }) : t('alerts.button')}
        onClick={() => setOpen(true)}
        className={cn('inline-flex shrink-0 items-center gap-1.5 rounded-md font-sans font-strong text-tx-1 outline-none transition-colors', className)}
      >
        <BellRing aria-hidden className="h-3.5 w-3.5 text-tx-3" />
        {t('alerts.button')}
        {worst && (
          <>
            {' '}
            <span className={cn('tabular-nums', SEVERITY_TEXT[worst])}>{going.length}</span>
          </>
        )}
      </button>
      <AlertsDrawer
        open={open}
        onClose={() => setOpen(false)}
        application={application}
        environment={environment}
        onOpenIncident={(id) => {
          setOpen(false);
          setViewing(id);
        }}
      />
      <IncidentDetailDrawer incidentId={viewing} onClose={() => setViewing(null)} />
    </>
  );
}

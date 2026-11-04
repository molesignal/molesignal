import { X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumApplication } from '@/api/rum';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';

import { GoingOn } from './GoingOn';
import { incidentsOf, rulesOf } from './model';
import { RulesOfApplication } from './Rules';
import { TemplateList } from './TemplateList';
import { useActiveIncidents, useAlertRules } from './useRumAlerts';

/**
 * The alerts of the application a page is of, in a drawer from the right: what is going on, which
 * rules watch it, and the templates that would add one. The application and the environment are the
 * page's, so the drawer follows them. It reads in that order, the most urgent at the top.
 */
export function AlertsDrawer({
  open,
  onClose,
  application,
  environment,
  onOpenIncident,
}: {
  open: boolean;
  onClose: () => void;
  application: RumApplication;
  environment: string;
  /** Opens an incident over the page; the drawer gives way to it. */
  onOpenIncident: (id: string) => void;
}) {
  const { t } = useTranslation('rum');

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        side="right"
        hideClose
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-rum-alerts-drawer=""
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-col gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width: 'min(520px, 100vw - 48px)' }}
      >
        <SheetTitle className="sr-only">{t('alerts.drawer.title')}</SheetTitle>
        {open && <DrawerBody application={application} environment={environment} onOpenIncident={onOpenIncident} />}
      </SheetContent>
    </Sheet>
  );
}

function DrawerBody({
  application,
  environment,
  onOpenIncident,
}: {
  application: RumApplication;
  environment: string;
  onOpenIncident: (id: string) => void;
}) {
  const { t } = useTranslation('rum');
  const manage = useActionAccess({ permission: 'alerts.rules.manage' });
  const incidents = useActiveIncidents(true);
  const rules = useAlertRules(true);
  const scope = { application: application.id, environment };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header className="flex shrink-0 items-start justify-between gap-4 border-b border-bd-0 px-5 py-4">
        <div className="min-w-0">
          <h2 className="m-0 truncate type-page-title font-sans font-display text-tx-0">{t('alerts.drawer.title')}</h2>
          <p className="m-0 truncate font-sans text-xs text-tx-3" data-rum-alerts-scope="">
            {[application.name, environment].filter(Boolean).join(' · ')} · {t('alerts.drawer.subtitle')}
          </p>
        </div>
        <SheetClose asChild>
          <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('alerts.drawer.close')}>
            <X className="h-4 w-4" />
          </Button>
        </SheetClose>
      </header>
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-5 py-4 font-sans">
        <GoingOn
          incidents={incidentsOf(incidents.data, scope)}
          rules={rules.data ?? []}
          pending={incidents.isPending}
          failed={incidents.isError}
          onRetry={() => void incidents.refetch()}
          onOpen={onOpenIncident}
        />
        <RulesOfApplication
          rules={rulesOf(rules.data, scope)}
          application={application.id}
          environment={environment}
          canManage={manage.allowed}
          pending={rules.isPending}
          failed={rules.isError}
          onRetry={() => void rules.refetch()}
        />
        <TemplateList application={application} environment={environment} rules={rules.data} ready={rules.isSuccess} manage={manage} />
      </div>
    </div>
  );
}

import { CalendarClock, ChevronRight, Plus, RadioTower } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { StatusPageIncident } from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton, Dot, Pill } from '@/shell/chrome';
import { PageBody } from '@/shell/PageHeader';

import { COMPONENT_TONE, componentStatusLabel, incidentStatusLabel } from '../model';
import {
  StatusPageCanvas,
  StatusPageKpiBand,
  StatusPageSection,
} from './CardlessSurface';
import { useStatusPageWorkspace } from './Layout';

export function StatusPageOverview() {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const activeComponents = snapshot.components.filter((component) => component.lifecycle === 'active');
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageKpiBand
          items={[
            {
              label: t('kpis.overall'),
              value: (
                <span className="flex items-center gap-2">
                  <Dot tone={snapshot.overall_status === 'operational' ? 'green' : 'orange'} />
                  {componentStatusLabel(t, snapshot.overall_status)}
                </span>
              ),
              tone: snapshot.overall_status === 'operational' ? 'good' : 'warn',
            },
            { label: t('kpis.components'), value: String(activeComponents.length) },
            { label: t('kpis.active_incidents'), value: String(snapshot.active_incidents.length) },
            { label: t('kpis.maintenance'), value: String(snapshot.scheduled_maintenance.length) },
          ]}
        />

        <div className="grid grid-cols-1 gap-8 py-6 xl:grid-cols-2">
          <StatusPageSection
            title={t('sections.components')}
            action={
              <button
                type="button"
                onClick={() => navigate(`/status-pages/${pageId}/components`)}
                className="rounded-md px-2 py-1 text-xs font-strong text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2"
              >
                {t('actions.manage_components')}
              </button>
            }
          >
            <div className="divide-y divide-bd-0">
              {activeComponents.slice(0, 6).map((component) => (
                <button
                  type="button"
                  key={component.id}
                  onClick={() => navigate(`/status-pages/${pageId}/components/${component.id}`)}
                  className="flex min-h-12 w-full items-center gap-3 px-4 text-left hover:bg-bg-2 focus-visible:bg-bg-2"
                >
                  <Dot tone={component.status === 'operational' ? 'green' : 'orange'} />
                  <span className="min-w-0 flex-1 truncate text-sm font-strong text-tx-0">
                    {component.name}
                  </span>
                  {component.visibility === 'hidden' && (
                    <Pill tone="dim">{t('component_visibility.hidden')}</Pill>
                  )}
                  <Pill tone={COMPONENT_TONE[component.status]}>
                    {componentStatusLabel(t, component.status)}
                  </Pill>
                  <ChevronRight className="h-3.5 w-3.5 text-tx-3" />
                </button>
              ))}
              {activeComponents.length === 0 && (
                <EmptyRow>{t('states.no_components_description')}</EmptyRow>
              )}
            </div>
          </StatusPageSection>

          <div className="grid min-w-0 content-start gap-8">
            <EventPanel
              title={t('sections.active_incidents')}
              icon={<RadioTower className="h-4 w-4" />}
              items={snapshot.active_incidents}
              empty={t('states.no_active_incidents')}
              onOpen={(event) => navigate(`/status-pages/${pageId}/incidents/${event.id}`)}
              action={
                <ChromeButton
                  size="sm"
                  variant="ghost"
                  disabled={!canMutate}
                  disabledReason={manageAccess.reason}
                  onClick={() => navigate(`/status-pages/${pageId}/incidents/new`)}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t('actions.new_incident')}
                </ChromeButton>
              }
            />
            <EventPanel
              title={t('sections.scheduled_maintenance')}
              icon={<CalendarClock className="h-4 w-4" />}
              items={snapshot.scheduled_maintenance}
              empty={t('states.no_scheduled_maintenance')}
              onOpen={(event) => navigate(`/status-pages/${pageId}/maintenance/${event.id}`)}
              action={
                <ChromeButton
                  size="sm"
                  variant="ghost"
                  disabled={!canMutate}
                  disabledReason={manageAccess.reason}
                  onClick={() => navigate(`/status-pages/${pageId}/maintenance/new`)}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t('actions.schedule_maintenance')}
                </ChromeButton>
              }
            />
          </div>
        </div>
      </StatusPageCanvas>
    </PageBody>
  );
}

function EventPanel({
  title,
  icon,
  items,
  empty,
  action,
  onOpen,
}: {
  title: string;
  icon: React.ReactNode;
  items: StatusPageIncident[];
  empty: string;
  action: React.ReactNode;
  onOpen: (event: StatusPageIncident) => void;
}) {
  const { t } = useTranslation('status-pages');
  return (
    <StatusPageSection
      title={
        <span className="flex items-center gap-2">
          <span className="text-tx-2">{icon}</span>
          {title}
        </span>
      }
      action={action}
    >
      <div className="divide-y divide-bd-0">
        {items.slice(0, 3).map((event) => (
          <button
            type="button"
            key={event.id}
            onClick={() => onOpen(event)}
            className="block min-h-14 w-full px-4 py-2.5 text-left hover:bg-bg-2 focus-visible:bg-bg-2"
          >
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate text-sm font-strong text-tx-0">
                {event.title}
              </span>
              <Pill tone="neutral">{incidentStatusLabel(t, event.status)}</Pill>
            </div>
            <div className="mt-1 text-xs tabular-nums text-tx-3">
              {formatMicrosActive(event.started_at)}
            </div>
          </button>
        ))}
        {items.length === 0 && <EmptyRow>{empty}</EmptyRow>}
      </div>
    </StatusPageSection>
  );
}

function EmptyRow({ children }: { children: React.ReactNode }) {
  return <div className="px-4 py-8 text-center text-xs text-tx-3">{children}</div>;
}

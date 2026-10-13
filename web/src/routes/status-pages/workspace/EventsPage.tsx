import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Navigate, useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type {
  StatusPageEventView,
  StatusPageIncident,
  StatusPageIncidentKind,
} from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { PageBody } from '@/shell/PageHeader';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import {
  IMPACT_TONE,
  INCIDENT_TONE,
  affectedComponentNames,
  impactLabel,
  incidentStatusLabel,
} from '../model';
import { StatusPageEventDrawer } from './EventDrawer';
import { useStatusPageWorkspace } from './Layout';
import {
  StatusPageCanvas,
  StatusPageList,
  statusPageFlatStateClassName,
  statusPageFlatTableClassName,
} from './Surfaces';

/**
 * The views of what is still open. What is over is not listed here a second
 * time: the History tab has every ended incident and maintenance, with a
 * search and filters these views never had.
 */
const VIEWS: Record<StatusPageIncidentKind, StatusPageEventView[]> = {
  incident: ['current', 'draft'],
  maintenance: ['upcoming', 'in_progress', 'draft'],
};

/** Links made before the ended views moved still lead to the same events. */
const ENDED_VIEW: Partial<Record<StatusPageEventView, StatusPageIncidentKind>> = {
  resolved: 'incident',
  completed: 'maintenance',
};

export function StatusPageEvents({ kind }: { kind: StatusPageIncidentKind }) {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const location = useLocation();
  const { eventId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const requestedView = searchParams.get('view') as StatusPageEventView | null;
  const defaultView: StatusPageEventView = kind === 'incident' ? 'current' : 'upcoming';
  const view = requestedView && VIEWS[kind].includes(requestedView)
    ? requestedView
    : defaultView;
  // An old link to the ended events of this kind: they are read on the History tab now.
  const movedToHistory = requestedView !== null && ENDED_VIEW[requestedView] === kind;
  const query = useQuery({
    queryKey: ['status-pages', pageId, 'events', kind, view],
    queryFn: () => statusPagesApi.listEvents(pageId, kind, view),
    enabled: !movedToHistory,
  });
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';
  const basePath = `/status-pages/${pageId}/${kind === 'incident' ? 'incidents' : 'maintenance'}`;
  const closeDrawer = () => navigate({ pathname: basePath, search: location.search }, { replace: true });

  if (movedToHistory) {
    return <Navigate to={`/status-pages/${pageId}/history?type=${kind}`} replace />;
  }

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageList
          head={
            <ViewTabs
              label={t('event_views_label')}
              value={view}
              onValueChange={(next) =>
                setSearchParams(next === defaultView ? {} : { view: next }, { replace: true })
              }
              items={VIEWS[kind].map((item) => ({
                key: item,
                label: t(`event_views.${item}`),
                // Only the view being read has been counted.
                ...(query.data?.view === item ? { count: query.data.total } : {}),
              }))}
            />
          }
          action={
            <ChromeButton
              variant="primary"
              disabled={!canMutate}
              disabledReason={manageAccess.reason}
              onClick={() => navigate({ pathname: `${basePath}/new`, search: location.search })}
            >
              <Plus className="h-3.5 w-3.5" />
              {t(kind === 'incident' ? 'actions.new_incident' : 'actions.schedule_maintenance')}
            </ChromeButton>
          }
        >
      {query.isLoading ? (
        <ProductState variant="loading" className={statusPageFlatStateClassName} />
      ) : query.isError ? (
        <ProductState variant="error" error={query.error} className={statusPageFlatStateClassName} />
      ) : (
          <DataTable
            className={statusPageFlatTableClassName}
            rows={query.data?.items ?? []}
            rowKey={(event) => event.id}
            emptyLabel={t('states.no_events_in_view')}
            onRowClick={(event) =>
              navigate({ pathname: `${basePath}/${event.id}`, search: location.search })
            }
            columns={[
              {
                key: 'title',
                header: t('columns.title'),
                cell: (event) => (
                  <div className="min-w-0">
                    <div className="truncate text-tx-0">
                      {event.title || t('values.untitled_draft')}
                    </div>
                    <div className="mt-0.5 truncate text-xs font-normal text-tx-2">
                      {affectedComponentNames(event, snapshot.components).join(', ') || t('values.no_components')}
                    </div>
                  </div>
                ),
              },
              {
                key: 'status',
                header: t('columns.status'),
                width: 150,
                cell: (event) => (
                  <Pill tone={INCIDENT_TONE[event.status]}>
                    {event.publication_state === 'draft'
                      ? t('publication_state.draft')
                      : incidentStatusLabel(t, event.status, event.kind)}
                  </Pill>
                ),
              },
              // Maintenance has one impact, "maintenance": a column that says so on every row says nothing.
              ...(kind === 'incident'
                ? [
                    {
                      key: 'impact',
                      header: t('columns.impact'),
                      width: 140,
                      cell: (event: StatusPageIncident) => (
                        <Pill tone={IMPACT_TONE[event.impact]}>{impactLabel(t, event.impact)}</Pill>
                      ),
                    },
                  ]
                : []),
              {
                key: 'started',
                header: t(kind === 'incident' ? 'columns.started' : 'columns.scheduled_for'),
                width: 180,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(event.started_at)}</span>
                ),
              },
              {
                key: 'updated',
                header: t('columns.updated'),
                width: 180,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(event.updated_at)}</span>
                ),
              },
            ]}
          />
      )}
        </StatusPageList>
      </StatusPageCanvas>

      <StatusPageEventDrawer kind={kind} eventId={eventId} onClose={closeDrawer} />
    </PageBody>
  );
}

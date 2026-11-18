import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { OverviewPage } from '@/product/templates';
import { CriticalAlertBanner } from '@/shell/chrome';
import { rowsOf } from '@/shell/notifications/model';

import { formatAge } from './canvas/format';
import { OperationsCanvas } from './canvas/layout';
import { QuickStartDrawer } from './QuickStartDrawer';
import { NewMenu } from './workbench/NewMenu';
import { OnCallCard } from './workbench/OnCallCard';
import { RecentCard } from './workbench/RecentCard';
import { ResourcesCard } from './workbench/resources/ResourcesCard';
import { TodoCard, useIncidents } from './workbench/TodoCard';
import { UsageCard } from './workbench/UsageCard';
import { useActivation } from './workbench/useActivation';
import './workbench/workbench.css';

/** The most incidents the banner names; the rest are behind "view all". */
const BANNER_LIMIT = 5;

/**
 * The workbench: what the reader opened lately and the resources that are theirs on the left, what
 * waits on them, their on-call and the month's intake on the right. Only a critical incident nobody
 * has answered stands above it all.
 */
export function Home() {
  const { t } = useTranslation('onboarding');
  const nav = useNavigate();
  const [quickStartOpen, setQuickStartOpen] = React.useState(false);
  const activation = useActivation();
  const incidents = useIncidents();
  const critical = rowsOf(incidents.data).filter(
    (incident) => incident.status === 'open' && incident.severity === 'critical',
  );

  return (
    <OverviewPage
      title={t('workbench.title')}
      appearance="surface"
      headerCompact
      bodyClassName="pb-[24px] pt-0"
      toolbar={<NewMenu />}
    >
      <OperationsCanvas>
        {critical.length > 0 && (
          <div className="rounded-md bg-[var(--functional-surface)] p-[12px] [box-shadow:var(--shadow-functional-surface)]">
            <CriticalAlertBanner
              title={t('home.critical.title', { count: critical.length })}
              items={critical.slice(0, BANNER_LIMIT).map((incident) => {
                const service = incident.affectedServices[0];
                const age = formatAge(incident.createdAt);
                return {
                  id: incident.id,
                  label: incident.summary || incident.id,
                  meta: service ? `${service} · ${age}` : age,
                };
              })}
              viewAllLabel={t('home.view_all')}
              onViewAll={() => nav('/alerts')}
            />
          </div>
        )}
        <div className="wb-grid" data-testid="workbench">
          <div className="wb-column">
            <RecentCard />
            <ResourcesCard className="wb-fill" />
          </div>
          <div className="wb-column">
            <TodoCard
              className="wb-todos"
              activation={activation.todo}
              onOpenActivation={() => setQuickStartOpen(true)}
            />
            <OnCallCard className="wb-fixed" />
            <UsageCard className="wb-fixed" />
          </div>
        </div>
      </OperationsCanvas>
      <QuickStartDrawer
        open={quickStartOpen}
        onOpenChange={setQuickStartOpen}
        state={activation.state}
        onOpenStep={(to) => {
          setQuickStartOpen(false);
          nav(to);
        }}
        onLoadSample={() =>
          activation.loadSample.mutate(undefined, {
            onSuccess: (result) => {
              if (result.dashboardId) nav(`/dashboards/${encodeURIComponent(result.dashboardId)}`);
            },
          })
        }
        loadingSample={activation.loadSample.isPending}
      />
    </OverviewPage>
  );
}

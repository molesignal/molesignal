import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  BookOpen,
  Bot,
  ClipboardCheck,
  Gauge,
  ListTodo,
  MessageSquareText,
  Settings2,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, Outlet, useLocation } from 'react-router-dom';

import * as agentApi from '@/api/agent';
import { hasPermission, useProductAccess } from '@/product/access';
import { cn } from '@/shell/lib/cn';
import {
  surfacePageRootClass,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import { KNOWLEDGE_VIEWS, knowledgeViewOf } from './knowledge/KnowledgePage';
import { QUALITY_VIEWS, qualityViewOf } from './quality/model';
import {
  AGENT_SETTINGS_SECTIONS,
  agentSettingsPath,
  agentSettingsSectionOf,
} from './settings/sections';

const MODULE_NAV = [
  { to: '/agent/chat', key: 'nav.chat', icon: MessageSquareText },
  { to: '/agent/investigations', key: 'nav.investigations', icon: ListTodo },
  { to: '/agent/knowledge', key: 'nav.knowledge', icon: BookOpen },
  { to: '/agent/automations', key: 'nav.automations', icon: Workflow },
  { to: '/agent/quality', key: 'nav.quality', icon: Gauge },
  { to: '/agent/approvals', key: 'nav.approvals', icon: ClipboardCheck },
  { to: '/agent/executions', key: 'nav.executions', icon: Activity },
  { to: '/agent/settings', key: 'nav.settings', icon: Settings2 },
] as const;

export function AgentLayout() {
  const { t } = useTranslation('agent');
  const { pathname } = useLocation();
  const settingsSection = agentSettingsSectionOf(pathname);
  const knowledgeView = knowledgeViewOf(pathname);
  const qualityView = qualityViewOf(pathname);
  // The Quality tab reads team chats: it is there for those who may read them all.
  const access = useProductAccess();
  const nav = MODULE_NAV.filter(
    (item) => item.to !== '/agent/quality' || hasPermission('agent.quality.read', access),
  );
  const overview = useQuery({
    queryKey: ['agent', 'overview'],
    queryFn: agentApi.overview,
    retry: false,
    refetchInterval: 30_000,
  });
  const loadingValue = overview.isPending ? '…' : '—';

  return (
    <section
      data-page-appearance="surface"
      className={cn(
        'flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col',
        surfacePageRootClass,
      )}
    >
      <header
        data-testid="agent-module-header"
        className="shrink-0 bg-[var(--page-canvas)]"
      >
        <div data-testid="agent-title-row">
          <SurfacePageHeader
            compact
            moduleIcon={Bot}
            moduleIconTestId="agent-module-icon"
            title="Mole Agent"
            subtitle={t('module_subtitle')}
            toolbar={
              <div className="flex shrink-0 flex-nowrap items-center gap-2" aria-label={t('overview.label')}>
                <StatusTag
                  label={t('overview.active')}
                  value={overview.data?.activeInvestigations ?? loadingValue}
                  to="/agent/investigations?status=running"
                  active={Boolean(overview.data?.activeInvestigations)}
                />
                <StatusTag
                  label={t('overview.pending_approvals')}
                  value={overview.data?.pendingApprovals ?? loadingValue}
                  to="/agent/approvals?status=pending"
                  attention={Boolean(overview.data?.pendingApprovals)}
                />
              </div>
            }
          />
        </div>

        <ModuleTabs
          label={t('nav.label')}
          items={nav.map((item) => ({
            key: item.to,
            to: item.to,
            icon: item.icon,
            label: t(item.key),
          }))}
        >
          {settingsSection && (
            // Which part of the settings is open is a level of the navigation, said where the
            // tabs are: inside the page it stood under two titles and over a third.
            <div className="border-t border-bd-0">
              <div
                data-agent-subnavigation="settings"
                className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
              >
                <ViewTabs
                  label={t('settings.title')}
                  items={AGENT_SETTINGS_SECTIONS.map((section) => ({
                    key: section.value,
                    to: agentSettingsPath(section.value),
                    label: t(section.key),
                    active: section.value === settingsSection,
                  }))}
                />
              </div>
            </div>
          )}
          {qualityView && (
            <div className="border-t border-bd-0">
              <div
                data-agent-subnavigation="quality"
                className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
              >
                <ViewTabs
                  label={t('nav.quality')}
                  items={QUALITY_VIEWS.map((view) => ({
                    key: view.value,
                    to: view.path,
                    label: t(view.key),
                    active: view.value === qualityView,
                  }))}
                />
              </div>
            </div>
          )}
          {knowledgeView && (
            <div className="border-t border-bd-0">
              <div
                data-agent-subnavigation="knowledge"
                className="flex h-[40px] items-center px-[10px] [@media(pointer:coarse)]:h-[44px]"
              >
                <ViewTabs
                  label={t('nav.knowledge')}
                  items={KNOWLEDGE_VIEWS.map((view) => ({
                    key: view.value,
                    to: view.path,
                    label: t(view.key),
                    active: view.value === knowledgeView,
                  }))}
                />
              </div>
            </div>
          )}
        </ModuleTabs>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden px-[20px] pb-[20px]">
        <div
          data-agent-content-surface
          className="h-full min-h-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
        >
          <Outlet />
        </div>
      </div>
    </section>
  );
}

function StatusTag({
  label,
  value,
  to,
  active = false,
  attention = false,
}: {
  label: string;
  value: number | string;
  to: string;
  active?: boolean;
  attention?: boolean;
}) {
  return (
    <Link
      to={to}
      className="inline-flex h-7 items-center gap-2 rounded-full bg-bg-2 px-2.5 type-caption text-tx-2 transition-colors duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
    >
      <span
        className={cn(
          'font-display-strong tabular-nums text-tx-0',
          active && 'text-indigo',
          attention && 'text-[var(--orange-ink)]',
        )}
      >
        {value}
      </span>
      <span>{label}</span>
    </Link>
  );
}

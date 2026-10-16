import { useMutation, useQuery } from '@tanstack/react-query';
import { Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as businessServicesApi from '@/api/businessServices';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageComponent, StatusPageComponentInput } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { PageBody } from '@/shell/PageHeader';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { COMPONENT_STATUSES, componentStatusLabel, isComponentGroup, namedComponents } from '../model';
import { componentColumns } from './component/columns';
import { ComponentFormDrawer, type ComponentSubmission } from './component/ComponentFormDrawer';
import { componentOrder, componentRows, componentSiblings } from './component/rows';
import { useStatusPageWorkspace } from './Layout';
import {
  StatusPageCanvas,
  StatusPageList,
  statusPageFlatTableClassName,
} from './Surfaces';

/**
 * The components of a page, in the order the public page lists them: groups
 * with their components under them, the way folders hold files.
 */
export function StatusPageComponents() {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const { componentId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot, manageAccess, refresh } = useStatusPageWorkspace();
  const [deleting, setDeleting] = React.useState<StatusPageComponent | null>(null);
  const lifecycle = searchParams.get('lifecycle') === 'archived' ? 'archived' : 'active';
  const search = searchParams.get('q') ?? '';
  const status = COMPONENT_STATUSES.find((item) => item === searchParams.get('status'));
  const visibility = (['enabled', 'hidden'] as const).find(
    (item) => item === searchParams.get('visibility'),
  );
  const creating = componentId === 'new';
  const editing = snapshot.components.find((item) => item.id === componentId) ?? null;
  const rows = componentRows(snapshot.components, { lifecycle, search, status, visibility });
  const groups = snapshot.components
    .filter((item) => isComponentGroup(item) && item.lifecycle === 'active')
    .sort(componentOrder);
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';
  const businessServiceAccess = useActionAccess({ permission: 'business_services.read' });
  // The column is there only when a component on the page stands for a business service.
  const linksShown = businessServiceAccess.allowed
    && snapshot.components.some((component) => component.business_service);
  const servicesQuery = useQuery({
    queryKey: ['business-services'],
    queryFn: businessServicesApi.list,
    enabled: linksShown,
    staleTime: 30_000,
  });
  const hasHidden = namedComponents(snapshot.components).some(
    (item) => item.lifecycle === lifecycle && item.visibility === 'hidden',
  );
  const closeDrawer = () => navigate(`/status-pages/${pageId}/components`, { replace: true });
  const updateFilter = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    setSearchParams(next, { replace: true });
  };

  const saveMutation = useMutation({
    mutationFn: async ({ input, link }: ComponentSubmission) => {
      const saved = editing
        ? await statusPagesApi.updateComponent(pageId, editing.id, input)
        : await statusPagesApi.createComponent(pageId, input);
      if (link) return statusPagesApi.linkComponentService(pageId, saved.id, link);
      if (link === null && saved.business_service) {
        return statusPagesApi.unlinkComponentService(pageId, saved.id);
      }
      return saved;
    },
    onSuccess: async () => {
      toast.success(t('toast.component_saved'));
      closeDrawer();
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const lifecycleMutation = useMutation({
    mutationFn: ({ component, next }: { component: StatusPageComponent; next: 'active' | 'archived' }) =>
      statusPagesApi.updateComponent(pageId, component.id, componentInput(component, next)),
    onSuccess: async (_, variables) => {
      toast.success(t(variables.next === 'active' ? 'toast.component_restored' : 'toast.component_archived'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const orderMutation = useMutation({
    mutationFn: async ({
      component,
      direction,
    }: {
      component: StatusPageComponent;
      direction: -1 | 1;
    }) => {
      // A component moves among the others of its group; a group among the top level.
      const siblings = componentSiblings(snapshot.components, component);
      const index = siblings.findIndex((item) => item.id === component.id);
      const neighbor = siblings[index + direction];
      if (!neighbor) return;
      await Promise.all([
        statusPagesApi.updateComponent(
          pageId,
          component.id,
          componentInput(component, 'active', neighbor.position),
        ),
        statusPagesApi.updateComponent(
          pageId,
          neighbor.id,
          componentInput(neighbor, 'active', component.position),
        ),
      ]);
    },
    onSuccess: async () => {
      toast.success(t('toast.component_reordered'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const deleteMutation = useMutation({
    mutationFn: (id: string) => statusPagesApi.removeComponent(pageId, id),
    onSuccess: async () => {
      toast.success(t('toast.component_deleted'));
      setDeleting(null);
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const columns = componentColumns({
    t,
    serviceNames: linksShown
      ? new Map((servicesQuery.data ?? []).map((service) => [service.id, service.name]))
      : null,
    actions: {
      canMutate,
      disabledReason: manageAccess.reason,
      busy: { ordering: orderMutation.isPending, archiving: lifecycleMutation.isPending },
      siblingsOf: (component) => componentSiblings(snapshot.components, component),
      onMove: (component, direction) => orderMutation.mutate({ component, direction }),
      onEdit: (component) => navigate(`/status-pages/${pageId}/components/${component.id}`),
      onArchive: (component) => lifecycleMutation.mutate({ component, next: 'archived' }),
      onRestore: (component) => lifecycleMutation.mutate({ component, next: 'active' }),
      onDelete: setDeleting,
    },
  });

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageList
          head={
            <>
              <ViewTabs
                label={t('fields.component_lifecycle')}
                value={lifecycle}
                onValueChange={(next) => updateFilter('lifecycle', next === 'archived' ? 'archived' : '')}
                items={(['active', 'archived'] as const).map((item) => ({
                  key: item,
                  label: t(`lifecycle.${item}`),
                  count: snapshot.components.filter((component) => component.lifecycle === item).length,
                }))}
              />
              <label className="relative w-full min-w-0 sm:w-[260px]">
                <span className="sr-only">{t('filters.search_components')}</span>
                <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
                <FormInput
                  value={search}
                  onChange={(event) => updateFilter('q', event.currentTarget.value)}
                  placeholder={t('filters.component_search_placeholder')}
                  className="pl-9"
                />
              </label>
              <FormSelect
                ariaLabel={t('filters.status')}
                className="w-full sm:w-[210px]"
                value={status ?? ''}
                onChange={(value) => updateFilter('status', value)}
                options={[
                  { value: '', label: t('filters.all_component_statuses') },
                  ...COMPONENT_STATUSES.map((item) => ({
                    value: item,
                    label: componentStatusLabel(t, item),
                  })),
                ]}
              />
              {/* A filter for something no component is would only ever empty the list. */}
              {(hasHidden || visibility) && (
                <FormSelect
                  ariaLabel={t('filters.visibility')}
                  className="w-full sm:w-[170px]"
                  value={visibility ?? ''}
                  onChange={(value) => updateFilter('visibility', value)}
                  options={[
                    { value: '', label: t('filters.all_visibilities') },
                    { value: 'enabled', label: t('component_visibility.enabled') },
                    { value: 'hidden', label: t('component_visibility.hidden') },
                  ]}
                />
              )}
            </>
          }
          action={
            <ChromeButton
              variant="primary"
              disabled={!canMutate}
              disabledReason={manageAccess.reason}
              onClick={() => navigate(`/status-pages/${pageId}/components/new`)}
            >
              <Plus className="h-3.5 w-3.5" />
              {t('actions.add_component')}
            </ChromeButton>
          }
        >
          <DataTable
            className={statusPageFlatTableClassName}
            rows={rows}
            rowKey={(row) => row.component.id}
            emptyLabel={t(lifecycle === 'active' ? 'states.no_components_title' : 'states.no_archived_components')}
            {...(lifecycle === 'active'
              ? {
                  onRowClick: ({ component }: (typeof rows)[number]) =>
                    navigate(`/status-pages/${pageId}/components/${component.id}`),
                }
              : {})}
            columns={columns}
          />
        </StatusPageList>
      </StatusPageCanvas>

      <ComponentFormDrawer
        open={creating || Boolean(editing)}
        component={editing}
        groups={groups}
        busy={saveMutation.isPending}
        onOpenChange={(open) => !open && closeDrawer()}
        onSubmit={(submission) => saveMutation.mutate(submission)}
      />
      <ConfirmDialog
        open={Boolean(deleting)}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('confirm.delete_component_title')}
        description={t('confirm.delete_component_description', { name: deleting?.name })}
        confirmLabel={t('actions.delete')}
        destructive
        busy={deleteMutation.isPending}
        onConfirm={() => deleting && deleteMutation.mutate(deleting.id)}
      />
    </PageBody>
  );
}

function componentInput(
  component: StatusPageComponent,
  lifecycle: 'active' | 'archived',
  position = component.position,
): StatusPageComponentInput {
  return {
    name: component.name,
    description: component.description,
    status: component.status,
    visibility: component.visibility,
    lifecycle,
    position,
    kind: component.kind ?? 'component',
    group_id: component.group_id ?? null,
  };
}

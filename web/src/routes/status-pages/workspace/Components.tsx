import { useMutation } from '@tanstack/react-query';
import {
  Archive,
  ArchiveRestore,
  ArrowDown,
  ArrowUp,
  Pencil,
  Plus,
  Search,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageComponent, StatusPageComponentInput } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { PageBody } from '@/shell/PageHeader';
import { toast } from '@/shell/ui/sonner';

import { ComponentFormDrawer } from '../ConfigurationDrawers';
import { COMPONENT_STATUSES, COMPONENT_TONE, componentStatusLabel } from '../model';
import { useStatusPageWorkspace } from './Layout';

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
  const visibility = ['enabled', 'hidden'].find(
    (item) => item === searchParams.get('visibility'),
  );
  const creating = componentId === 'new';
  const editing = snapshot.components.find((item) => item.id === componentId) ?? null;
  const activeOrder = snapshot.components
    .filter((item) => item.lifecycle === 'active')
    .sort(componentOrder);
  const rows = snapshot.components
    .filter((item) => item.lifecycle === lifecycle)
    .filter((item) => !status || item.status === status)
    .filter((item) => !visibility || item.visibility === visibility)
    .filter((item) => {
      const normalized = search.trim().toLocaleLowerCase();
      return !normalized
        || item.name.toLocaleLowerCase().includes(normalized)
        || item.description.toLocaleLowerCase().includes(normalized);
    })
    .sort(componentOrder);
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';
  const closeDrawer = () => navigate(`/status-pages/${pageId}/components`, { replace: true });
  const updateFilter = (key: string, value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value) next.set(key, value);
    else next.delete(key);
    setSearchParams(next, { replace: true });
  };

  const saveMutation = useMutation({
    mutationFn: (input: StatusPageComponentInput) =>
      editing
        ? statusPagesApi.updateComponent(pageId, editing.id, input)
        : statusPagesApi.createComponent(pageId, input),
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
      const index = activeOrder.findIndex((item) => item.id === component.id);
      const neighbor = activeOrder[index + direction];
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

  return (
    <PageBody className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-1">
          {(['active', 'archived'] as const).map((item) => (
            <ChromeButton
              key={item}
              size="sm"
              variant={lifecycle === item ? 'primary' : 'ghost'}
              onClick={() => updateFilter('lifecycle', item === 'archived' ? 'archived' : '')}
            >
              {t(`lifecycle.${item}`)}
              <span className="type-micro font-mono opacity-70">
                {snapshot.components.filter((component) => component.lifecycle === item).length}
              </span>
            </ChromeButton>
          ))}
        </div>
        <ChromeButton
          variant="primary"
          className="ml-auto"
          disabled={!canMutate}
          disabledReason={manageAccess.reason}
          onClick={() => navigate(`/status-pages/${pageId}/components/new`)}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('actions.add_component')}
        </ChromeButton>
      </div>

      <div className="grid gap-2 rounded-lg border border-bd-0 bg-bg-1 p-3 md:grid-cols-[minmax(220px,1fr)_200px_180px]">
        <label className="relative min-w-0">
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
        <FormSelect
          ariaLabel={t('filters.visibility')}
          value={visibility ?? ''}
          onChange={(value) => updateFilter('visibility', value)}
          options={[
            { value: '', label: t('filters.all_visibilities') },
            { value: 'enabled', label: t('component_visibility.enabled') },
            { value: 'hidden', label: t('component_visibility.hidden') },
          ]}
        />
      </div>

      <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
        <DataTable
          rows={rows}
          rowKey={(component) => component.id}
          emptyLabel={t(lifecycle === 'active' ? 'states.no_components_title' : 'states.no_archived_components')}
          {...(lifecycle === 'active'
            ? { onRowClick: (component: StatusPageComponent) => navigate(`/status-pages/${pageId}/components/${component.id}`) }
            : {})}
          columns={[
            {
              key: 'name',
              header: t('columns.component'),
              cell: (component) => (
                <div className="min-w-0">
                  <div className="truncate text-tx-0">{component.name}</div>
                  <div className="mt-0.5 truncate text-xs font-normal text-tx-3">
                    {component.description || t('values.no_description')}
                  </div>
                </div>
              ),
            },
            {
              key: 'status',
              header: t('columns.status'),
              width: 180,
              cell: (component) => (
                <Pill tone={COMPONENT_TONE[component.status]}>
                  {componentStatusLabel(t, component.status)}
                </Pill>
              ),
            },
            {
              key: 'visibility',
              header: t('columns.visibility'),
              width: 120,
              cell: (component) => (
                <Pill tone={component.visibility === 'enabled' ? 'neutral' : 'dim'}>
                  {t(`component_visibility.${component.visibility}`)}
                </Pill>
              ),
            },
            {
              key: 'actions',
              header: t('columns.actions'),
              width: 220,
              cell: (component) => (
                <div className="flex items-center gap-0.5" onClick={(event) => event.stopPropagation()}>
                  {component.lifecycle === 'active' ? (
                    <>
                      <IconButton
                        aria-label={t('actions.move_up')}
                        title={t('actions.move_up')}
                        disabled={!canMutate || activeOrder[0]?.id === component.id || orderMutation.isPending}
                        disabledReason={manageAccess.reason}
                        onClick={() => orderMutation.mutate({ component, direction: -1 })}
                      >
                        <ArrowUp className="h-3.5 w-3.5" />
                      </IconButton>
                      <IconButton
                        aria-label={t('actions.move_down')}
                        title={t('actions.move_down')}
                        disabled={!canMutate || activeOrder.at(-1)?.id === component.id || orderMutation.isPending}
                        disabledReason={manageAccess.reason}
                        onClick={() => orderMutation.mutate({ component, direction: 1 })}
                      >
                        <ArrowDown className="h-3.5 w-3.5" />
                      </IconButton>
                      <IconButton
                        aria-label={t('actions.edit_component', { name: component.name })}
                        title={t('actions.edit_component', { name: component.name })}
                        disabled={!canMutate}
                        disabledReason={manageAccess.reason}
                        onClick={() => navigate(`/status-pages/${pageId}/components/${component.id}`)}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </IconButton>
                      <IconButton
                        aria-label={t('actions.archive')}
                        title={t('actions.archive')}
                        disabled={!canMutate || lifecycleMutation.isPending}
                        disabledReason={manageAccess.reason}
                        onClick={() => lifecycleMutation.mutate({ component, next: 'archived' })}
                      >
                        <Archive className="h-3.5 w-3.5" />
                      </IconButton>
                    </>
                  ) : (
                    <>
                      <IconButton
                        aria-label={t('actions.restore')}
                        title={t('actions.restore')}
                        disabled={!canMutate || lifecycleMutation.isPending}
                        disabledReason={manageAccess.reason}
                        onClick={() => lifecycleMutation.mutate({ component, next: 'active' })}
                      >
                        <ArchiveRestore className="h-3.5 w-3.5" />
                      </IconButton>
                      <IconButton
                        aria-label={t('actions.delete')}
                        title={t('actions.delete')}
                        disabled={!canMutate}
                        disabledReason={manageAccess.reason}
                        className="enabled:hover:text-red-soft"
                        onClick={() => setDeleting(component)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </IconButton>
                    </>
                  )}
                </div>
              ),
            },
          ]}
        />
      </div>

      <ComponentFormDrawer
        open={creating || Boolean(editing)}
        component={editing}
        busy={saveMutation.isPending}
        onOpenChange={(open) => !open && closeDrawer()}
        onSubmit={(input) => saveMutation.mutate(input)}
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
  };
}

function componentOrder(left: StatusPageComponent, right: StatusPageComponent): number {
  return left.position - right.position || left.name.localeCompare(right.name) || left.id.localeCompare(right.id);
}

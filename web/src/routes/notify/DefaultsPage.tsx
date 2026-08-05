import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import * as teamsApi from '@/api/teams';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { NOTIFY_CATEGORIES, targetTypeOptions } from './model';
import { NotifySettingsPage } from './SettingsPage';

const ORGANIZATION_SCOPE = 'organization';

function normalizedRoutes(
  routes: notifyApi.NotifyDefaultRoute[],
): notifyApi.NotifyDefaultRoute[] {
  return routes.map((route, index) => ({ ...route, order: index + 1 }));
}

export function NotifyDefaultsPage() {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const manage = useActionAccess({ permission: 'alerts.manage' });
  const [scope, setScope] = React.useState(ORGANIZATION_SCOPE);
  const [category, setCategory] =
    React.useState<notifyApi.NotifyCategory>('alert');
  const [routes, setRoutes] = React.useState<notifyApi.NotifyDefaultRoute[]>([]);
  const [enabled, setEnabled] = React.useState(true);

  const teams = useQuery({
    queryKey: ['iam', 'teams'],
    queryFn: teamsApi.list,
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const organizationDefaults = useQuery({
    queryKey: ['notify', 'organization-defaults'],
    queryFn: notifyApi.listOrganizationDefaults,
  });
  const teamDefaults = useQuery({
    queryKey: ['notify', 'team-defaults', scope],
    queryFn: () => notifyApi.listTeamDefaults(scope),
    enabled: scope !== ORGANIZATION_SCOPE,
  });
  const records =
    scope === ORGANIZATION_SCOPE
      ? organizationDefaults.data ?? []
      : teamDefaults.data ?? [];
  const current = records.find((record) => record.category === category);

  React.useEffect(() => {
    setRoutes(current?.routes.slice().sort((a, b) => a.order - b.order) ?? []);
    setEnabled(current?.enabled ?? true);
  }, [current, category, scope]);

  const save = useMutation({
    mutationFn: () => {
      const prepared = normalizedRoutes(routes);
      return scope === ORGANIZATION_SCOPE
        ? notifyApi.updateOrganizationDefault(category, prepared, enabled)
        : notifyApi.updateTeamDefault(scope, category, prepared, enabled);
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'organization-defaults'] });
      void qc.invalidateQueries({ queryKey: ['notify', 'team-defaults'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: () =>
      scope === ORGANIZATION_SCOPE
        ? notifyApi.removeOrganizationDefault(category)
        : notifyApi.removeTeamDefault(scope, category),
    onSuccess: () => {
      toast.success(t('common.deleted'));
      setRoutes([]);
      void qc.invalidateQueries({ queryKey: ['notify', 'organization-defaults'] });
      void qc.invalidateQueries({ queryKey: ['notify', 'team-defaults'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const connectorOptions = (connectors.data ?? []).filter(
    (connector) => connector.enabled,
  );
  const addRoute = () => {
    const connector = connectorOptions[0];
    if (!connector) return;
    setRoutes((value) => [
      ...value,
      {
        connector_id: connector.id,
        target_type: connector.capabilities.group
          ? 'fixed_group'
          : 'fixed_address',
        target: '',
        order: value.length + 1,
      },
    ]);
  };
  const updateRoute = (
    index: number,
    patch: Partial<notifyApi.NotifyDefaultRoute>,
  ) =>
    setRoutes((value) =>
      value.map((route, routeIndex) =>
        routeIndex === index ? { ...route, ...patch } : route,
      ),
    );

  return (
    <NotifySettingsPage
      title={t('defaults.title')}
      subtitle={t('defaults.subtitle')}
    >
      <div className="mx-auto w-full max-w-5xl space-y-5">
        <div className="grid gap-4 rounded-lg border border-bd-0 bg-bg-1 p-5 md:grid-cols-2">
          <FormField label={t('defaults.scope')}>
            <FormSelect
              value={scope}
              onChange={setScope}
              options={[
                {
                  value: ORGANIZATION_SCOPE,
                  label: t('defaults.organization_scope'),
                },
                ...(teams.data ?? []).map((team) => ({
                  value: team.id,
                  label: t('defaults.team_scope', { name: team.name }),
                })),
              ]}
            />
          </FormField>
          <FormField label={t('defaults.category')}>
            <FormSelect
              value={category}
              onChange={(value) =>
                setCategory(value as notifyApi.NotifyCategory)
              }
              options={NOTIFY_CATEGORIES.map((value) => ({
                value,
                label: t(`preferences.${value}`),
              }))}
            />
          </FormField>
        </div>

        <section className="rounded-lg border border-bd-0 bg-bg-1">
          <header className="flex min-h-14 flex-wrap items-center gap-3 border-b border-bd-0 px-5 py-3">
            <div className="min-w-0 flex-1">
              <h2 className="text-sm font-semibold text-tx-0">{t('defaults.routes')}</h2>
              <p className="mt-0.5 text-xs text-tx-3">
                {current ? t('defaults.configured_hint') : t('defaults.empty')}
              </p>
            </div>
            <label className="flex min-h-9 items-center gap-2 text-xs text-tx-1">
              <span>{t('defaults.enabled')}</span>
              <Switch checked={enabled} onCheckedChange={setEnabled} />
            </label>
          </header>
          <div className="space-y-3 p-5">
            {routes.map((route, index) => (
              <div
                key={`${index}:${route.connector_id}`}
                className="grid gap-3 rounded-md border border-bd-0 bg-bg-2 p-3 md:grid-cols-[36px_minmax(0,1fr)_180px_minmax(0,1fr)_36px]"
              >
                <div className="grid h-9 place-items-center rounded-md bg-bg-3 font-mono text-xs text-tx-2">
                  {index + 1}
                </div>
                <FormField label={t('defaults.connector')}>
                  <FormSelect
                    value={route.connector_id}
                    onChange={(value) =>
                      updateRoute(index, { connector_id: value })
                    }
                    options={connectorOptions.map((connector) => ({
                      value: connector.id,
                      label: connector.name,
                    }))}
                  />
                </FormField>
                <FormField label={t('defaults.target_type')}>
                  <FormSelect
                    value={route.target_type}
                    onChange={(value) =>
                      updateRoute(index, {
                        target_type: value as notifyApi.NotifyTargetType,
                      })
                    }
                    options={targetTypeOptions()
                      .filter((value) =>
                        value === 'fixed_address' || value === 'fixed_group',
                      )
                      .map((value) => ({
                        value,
                        label: t(`target_types.${value}`),
                      }))}
                  />
                </FormField>
                <FormField label={t('defaults.target')}>
                  <FormInput
                    value={route.target}
                    onChange={(event) =>
                      updateRoute(index, { target: event.target.value })
                    }
                  />
                </FormField>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <IconButton
                      className="self-end"
                      aria-label={t('defaults.remove_route')}
                      onClick={() =>
                        setRoutes((value) =>
                          normalizedRoutes(
                            value.filter((_, routeIndex) => routeIndex !== index),
                          ),
                        )
                      }
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </IconButton>
                  </TooltipTrigger>
                  <TooltipContent>{t('defaults.remove_route')}</TooltipContent>
                </Tooltip>
              </div>
            ))}
            <ChromeButton
              disabled={connectorOptions.length === 0}
              onClick={addRoute}
            >
              <Plus className="h-4 w-4" />
              {t('defaults.add_route')}
            </ChromeButton>
          </div>
          <footer className="flex justify-end gap-2 border-t border-bd-0 bg-bg-2 px-5 py-4">
            {current && (
              <ChromeButton
                disabled={manage.disabled || remove.isPending}
                disabledReason={manage.reason}
                onClick={() => remove.mutate()}
              >
                {t('defaults.delete')}
              </ChromeButton>
            )}
            <ChromeButton
              variant="primary"
              disabled={
                manage.disabled ||
                save.isPending ||
                routes.length === 0 ||
                routes.some((route) => route.target.trim() === '')
              }
              disabledReason={manage.reason}
              onClick={() => save.mutate()}
            >
              {save.isPending ? t('common.saving') : t('defaults.save')}
            </ChromeButton>
          </footer>
        </section>
      </div>
    </NotifySettingsPage>
  );
}

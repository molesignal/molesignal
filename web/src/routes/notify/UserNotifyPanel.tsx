import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDown, ArrowUp, Check, Pencil, Plus, Send, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { EndpointEditor } from './EndpointEditor';
import { connectorName } from './model';

const CATEGORIES: notifyApi.NotifyCategory[] = ['alert', 'oncall', 'report'];

export function UserNotifyPanel({ userId }: { userId: string }) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const verifyAccess = useActionAccess({ permission: 'org.members.manage' });
  const [editorOpen, setEditorOpen] = React.useState(false);
  const [editing, setEditing] = React.useState<notifyApi.UserNotifyEndpoint | null>(null);
  const [removing, setRemoving] = React.useState<notifyApi.UserNotifyEndpoint | null>(null);
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const endpoints = useQuery({
    queryKey: ['notify', 'endpoints', userId],
    queryFn: () => notifyApi.listEndpoints(userId),
  });
  const preferences = useQuery({
    queryKey: ['notify', 'preferences', userId],
    queryFn: () => notifyApi.listPreferences(userId),
  });
  const remove = useMutation({
    mutationFn: (id: string) => notifyApi.removeEndpoint(userId, id),
    onSuccess: () => {
      toast.success(t('common.deleted'));
      setRemoving(null);
      invalidate(qc, userId);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const test = useMutation({
    mutationFn: (id: string) => notifyApi.testEndpoint(userId, id),
    onSuccess: (result) => {
      if (result.sent) toast.success(t('common.test_sent'));
      else toast.error(result.error ?? t('common.test_failed'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const verify = useMutation({
    mutationFn: (id: string) => notifyApi.verifyEndpoint(userId, id),
    onSuccess: () => {
      toast.success(t('common.verified'));
      invalidate(qc, userId);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (connectors.isLoading || endpoints.isLoading || preferences.isLoading) {
    return <ProductState variant="loading" compact />;
  }
  if (connectors.isError || endpoints.isError || preferences.isError) {
    return (
      <ProductState
        variant="error"
        compact
        error={connectors.error ?? endpoints.error ?? preferences.error}
      />
    );
  }
  const connectorRows = connectors.data ?? [];
  const endpointRows = endpoints.data ?? [];
  const preferenceRows = preferences.data ?? [];

  return (
    <div className="space-y-8">
      <section>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h3 className="text-sm font-semibold text-tx-0">{t('endpoints.title')}</h3>
            <p className="mt-1 text-xs leading-relaxed text-tx-2">{t('endpoints.description')}</p>
          </div>
          <ChromeButton
            className="h-11 md:h-9"
            onClick={() => {
              setEditing(null);
              setEditorOpen(true);
            }}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('endpoints.add')}
          </ChromeButton>
        </div>
        <div className="mt-4 divide-y divide-bd-0 rounded-md border border-bd-0 bg-bg-1">
          {endpointRows.length === 0 && (
            <div className="px-4 py-8 text-center text-sm text-tx-3">{t('endpoints.empty')}</div>
          )}
          {endpointRows.map((endpoint) => (
            <div key={endpoint.id} className="flex min-h-16 items-center gap-3 px-3 py-3">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-tx-0">
                  {endpoint.display_name ?? connectorName(connectorRows, endpoint.connector_id)}
                </div>
                <div className="mt-0.5 truncate font-mono text-xs text-tx-2">
                  {endpoint.external_identity}
                </div>
              </div>
              <Pill tone={endpoint.verified ? 'green' : 'yellow'}>
                {t(endpoint.verified ? 'common.verified' : 'common.unverified')}
              </Pill>
              <Pill tone={endpoint.enabled ? 'blue' : 'dim'}>
                {t(endpoint.enabled ? 'common.enabled' : 'common.disabled')}
              </Pill>
              <div className="flex shrink-0 items-center gap-0.5">
                <EndpointAction
                  label={t('common.test')}
                  icon={<Send className="h-3.5 w-3.5" />}
                  disabled={!endpoint.enabled || test.isPending}
                  onClick={() => test.mutate(endpoint.id)}
                />
                {!endpoint.verified && verifyAccess.allowed && (
                  <EndpointAction
                    label={t('common.verify')}
                    icon={<Check className="h-3.5 w-3.5" />}
                    disabled={verify.isPending}
                    onClick={() => verify.mutate(endpoint.id)}
                  />
                )}
                <EndpointAction
                  label={t('common.edit')}
                  icon={<Pencil className="h-3.5 w-3.5" />}
                  onClick={() => {
                    setEditing(endpoint);
                    setEditorOpen(true);
                  }}
                />
                <EndpointAction
                  label={t('common.delete')}
                  icon={<Trash2 className="h-3.5 w-3.5" />}
                  onClick={() => setRemoving(endpoint)}
                />
              </div>
            </div>
          ))}
        </div>
      </section>

      <section>
        <h3 className="text-sm font-semibold text-tx-0">{t('preferences.title')}</h3>
        <p className="mt-1 text-xs leading-relaxed text-tx-2">{t('preferences.description')}</p>
        <div className="mt-4 space-y-3">
          {CATEGORIES.map((category) => (
            <PreferenceCard
              key={category}
              userId={userId}
              category={category}
              endpoints={endpointRows}
              connectors={connectorRows}
              preference={preferenceRows.find((value) => value.category === category)}
            />
          ))}
        </div>
      </section>

      <EndpointEditor
        open={editorOpen}
        userId={userId}
        endpoint={editing}
        connectors={connectorRows}
        canVerify={verifyAccess.allowed}
        onClose={() => {
          setEditorOpen(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('endpoints.delete_title')}
        description={t('endpoints.delete_description')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </div>
  );
}

function PreferenceCard({
  userId,
  category,
  endpoints,
  connectors,
  preference,
}: {
  userId: string;
  category: notifyApi.NotifyCategory;
  endpoints: notifyApi.UserNotifyEndpoint[];
  connectors: notifyApi.NotifyConnector[];
  preference: notifyApi.UserNotifyPreference | undefined;
}) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const quiet = preference?.quiet_hours;
  const [enabled, setEnabled] = React.useState(preference?.enabled ?? true);
  const [selected, setSelected] = React.useState<string[]>([]);
  const [quietEnabled, setQuietEnabled] = React.useState(Boolean(quiet?.enabled));
  const [timezone, setTimezone] = React.useState(String(quiet?.timezone ?? 'UTC'));
  const [start, setStart] = React.useState(String(quiet?.start ?? '22:00'));
  const [end, setEnd] = React.useState(String(quiet?.end ?? '08:00'));
  const [criticalBypass, setCriticalBypass] = React.useState(
    preference?.allow_critical_bypass ?? true,
  );
  React.useEffect(() => {
    const nextQuiet = preference?.quiet_hours;
    setEnabled(preference?.enabled ?? true);
    setSelected(
      (preference?.steps ?? [])
        .slice()
        .sort((left, right) => left.step_order - right.step_order)
        .map((step) => step.endpoint_id),
    );
    setQuietEnabled(Boolean(nextQuiet?.enabled));
    setTimezone(String(nextQuiet?.timezone ?? 'UTC'));
    setStart(String(nextQuiet?.start ?? '22:00'));
    setEnd(String(nextQuiet?.end ?? '08:00'));
    setCriticalBypass(preference?.allow_critical_bypass ?? true);
  }, [preference]);
  const save = useMutation({
    mutationFn: () =>
      notifyApi.updatePreference(userId, category, {
        enabled,
        endpoint_ids: selected,
        quiet_hours: quietEnabled
          ? { enabled: true, timezone, start, end }
          : null,
        allow_critical_bypass: criticalBypass,
      }),
    onSuccess: () => {
      toast.success(t('common.saved'));
      invalidate(qc, userId);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const toggleEndpoint = (id: string) => {
    setSelected((current) =>
      current.includes(id) ? current.filter((value) => value !== id) : [...current, id],
    );
  };
  const move = (index: number, direction: -1 | 1) => {
    setSelected((current) => {
      const target = index + direction;
      if (target < 0 || target >= current.length) return current;
      const next = [...current];
      [next[index], next[target]] = [next[target]!, next[index]!];
      return next;
    });
  };

  return (
    <article className="rounded-md border border-bd-0 bg-bg-1 p-4">
      <div className="flex items-center justify-between gap-4">
        <h4 className="text-sm font-semibold text-tx-0">{t(`preferences.${category}`)}</h4>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>
      <div className="mt-4 grid gap-4 xl:grid-cols-2">
        <div>
          <div className="mb-2 text-xs font-semibold text-tx-2">{t('preferences.endpoint_order')}</div>
          {endpoints.length === 0 ? (
            <div className="rounded-md bg-bg-2 px-3 py-4 text-xs text-tx-3">{t('preferences.no_endpoints')}</div>
          ) : (
            <div className="space-y-1.5">
              {endpoints.map((endpoint) => {
                const index = selected.indexOf(endpoint.id);
                return (
                  <div
                    key={endpoint.id}
                    className="flex min-h-11 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-3"
                  >
                    <button
                      type="button"
                      className="min-w-0 flex-1 truncate text-left text-sm text-tx-1 hover:text-tx-0"
                      onClick={() => toggleEndpoint(endpoint.id)}
                    >
                      {index >= 0 ? `${index + 1}. ` : ''}
                      {connectorName(connectors, endpoint.connector_id)} · {endpoint.display_name ?? endpoint.external_identity}
                    </button>
                    {index >= 0 && (
                      <>
                        <IconButton aria-label={t('preferences.move_up')} disabled={index === 0} onClick={() => move(index, -1)}>
                          <ArrowUp className="h-3 w-3" />
                        </IconButton>
                        <IconButton aria-label={t('preferences.move_down')} disabled={index === selected.length - 1} onClick={() => move(index, 1)}>
                          <ArrowDown className="h-3 w-3" />
                        </IconButton>
                      </>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
        <div className="space-y-3">
          <label className="flex min-h-11 items-center justify-between gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
            <span>{t('preferences.quiet_enabled')}</span>
            <Switch checked={quietEnabled} onCheckedChange={setQuietEnabled} />
          </label>
          {quietEnabled && (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <FormField label={t('preferences.timezone')}>
                <FormInput className="h-11 md:h-9" value={timezone} onChange={(event) => setTimezone(event.target.value)} />
              </FormField>
              <FormField label={t('preferences.start')}>
                <FormInput className="h-11 md:h-9" value={start} onChange={(event) => setStart(event.target.value)} />
              </FormField>
              <FormField label={t('preferences.end')}>
                <FormInput className="h-11 md:h-9" value={end} onChange={(event) => setEnd(event.target.value)} />
              </FormField>
            </div>
          )}
          <label className="flex min-h-11 items-center justify-between gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
            <span>{t('preferences.critical_bypass')}</span>
            <Switch checked={criticalBypass} onCheckedChange={setCriticalBypass} />
          </label>
        </div>
      </div>
      <div className="mt-4 flex justify-end">
        <ChromeButton
          variant="primary"
          className="h-11 md:h-9"
          disabled={save.isPending}
          onClick={() => save.mutate()}
        >
          {save.isPending
            ? t('common.saving')
            : t('preferences.save', { category: t(`preferences.${category}`) })}
        </ChromeButton>
      </div>
    </article>
  );
}

function EndpointAction({
  label,
  icon,
  disabled,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <IconButton aria-label={label} disabled={disabled} onClick={onClick}>
          {icon}
        </IconButton>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function invalidate(
  queryClient: ReturnType<typeof useQueryClient>,
  userId: string,
) {
  void queryClient.invalidateQueries({ queryKey: ['notify', 'endpoints', userId] });
  void queryClient.invalidateQueries({ queryKey: ['notify', 'preferences', userId] });
  void queryClient.invalidateQueries({ queryKey: ['notify', 'users'] });
}

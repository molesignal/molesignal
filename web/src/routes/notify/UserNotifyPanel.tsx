import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Pencil, Plus, Send, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton, IconButton, Pill, uiSectionTitleClass } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { EndpointEditor } from './EndpointEditor';
import { connectorName } from './model';
import { PreferenceSection } from './PreferenceSection';

const CATEGORIES: notifyApi.NotifyCategory[] = ['alert', 'oncall', 'report'];

export function UserNotifyPanel({ userId }: { userId: string }) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const verifyAccess = useActionAccess({ permission: 'org.members.update' });
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
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className={uiSectionTitleClass}>{t('endpoints.title')}</h3>
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
        <div className="mt-4 divide-y divide-bd-0 border-y border-bd-0">
          {endpointRows.length === 0 && (
            <div className="px-4 py-8 text-center text-sm text-tx-3">{t('endpoints.empty')}</div>
          )}
          {endpointRows.map((endpoint) => (
            <div key={endpoint.id} className="flex min-h-16 items-center gap-3 px-3 py-3">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-tx-0">
                  {endpoint.displayName ?? connectorName(connectorRows, endpoint.connectorId)}
                </div>
                <div className="mt-0.5 truncate font-mono text-xs text-tx-2">
                  {endpoint.externalIdentity}
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
        <h3 className={uiSectionTitleClass}>{t('preferences.title')}</h3>
        <p className="mt-1 text-xs leading-relaxed text-tx-2">{t('preferences.description')}</p>
        <div className="mt-4 space-y-3">
          {CATEGORIES.map((category) => (
            <PreferenceSection
              key={category}
              userId={userId}
              category={category}
              endpoints={endpointRows}
              connectors={connectorRows}
              preference={preferenceRows.find((value) => value.category === category)}
              onSaved={() => invalidate(qc, userId)}
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

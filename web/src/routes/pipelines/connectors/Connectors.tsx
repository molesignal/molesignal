import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as connectorsApi from '@/api/connectors';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { QueryState, queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { ConnectorDrawer, type DrawerTarget } from './ConnectorDrawer';
import { ConnectorSection } from './ConnectorSection';
import { isConnectorKind, isDestinationKind, specOf, type Direction } from './kinds';

/**
 * The workspace's connectors: on one side what brings logs in from other
 * services, on the other where pipelines deliver what they write. They are
 * kept apart because they answer different questions, though one kind of
 * record holds both.
 */
export function Connectors() {
  const { t } = useTranslation('pipelines');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'pipelines.edit' });
  const [target, setTarget] = React.useState<DrawerTarget | null>(null);
  const [removing, setRemoving] = React.useState<connectorsApi.Connector | null>(null);

  const query = useQuery({ queryKey: ['connectors'], queryFn: () => connectorsApi.list() });
  const connectors = query.data ?? [];
  // An empty list is a page with two empty sections, not a state of its own.
  const state = queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: [null] });
  const sources = connectors.filter((item) => !isDestinationKind(item.kind));
  const destinations = connectors.filter((item) => isDestinationKind(item.kind));

  const remove = useMutation({
    mutationFn: (connector: connectorsApi.Connector) => connectorsApi.remove(connector.id),
    onSuccess: () => {
      toast.success(t('connectors.toast.deleted'));
      void queryClient.invalidateQueries({ queryKey: ['connectors'] });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const toggle = useMutation({
    mutationFn: ({ connector, enabled }: { connector: connectorsApi.Connector; enabled: boolean }) =>
      // The configuration goes back as it was read: masked secrets are kept by the backend.
      connectorsApi.update(connector.id, {
        name: connector.name,
        kind: connector.kind,
        config_json: connector.config_json,
        enabled,
      }),
    onSuccess: (_saved, { connector, enabled }) => {
      toast.success(t(enabled ? 'connectors.toast.enabled' : 'connectors.toast.disabled', { name: connector.name }));
      void queryClient.invalidateQueries({ queryKey: ['connectors'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const section = (direction: Direction, rows: connectorsApi.Connector[]) => (
    <ConnectorSection
      direction={direction}
      connectors={rows}
      access={access}
      busyId={toggle.isPending ? (toggle.variables?.connector.id ?? null) : null}
      onAdd={() => setTarget({ mode: 'new', direction })}
      onEdit={(connector) => setTarget({ mode: 'edit', connector })}
      onRemove={setRemoving}
      onToggle={(connector, enabled) => toggle.mutate({ connector, enabled })}
    />
  );
  const removingDirection: Direction =
    removing && isConnectorKind(removing.kind) ? specOf(removing.kind).direction : 'source';

  return (
    <>
      <PageHeader
        title={t('connectors.title')}
        subtitle={t('connectors.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={access.disabled}
            disabledReason={access.reason}
            onClick={() => setTarget({ mode: 'new' })}
          >
            <Plus aria-hidden className="h-3.5 w-3.5" />
            {t('connectors.new')}
          </ChromeButton>
        }
      />
      <ConnectorDrawer target={target} access={access} onClose={() => setTarget(null)} />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('connectors.remove.title', { name: removing?.name ?? '' })}
        description={t(`connectors.remove.${removingDirection}`)}
        confirmLabel={t('connectors.remove.confirm')}
        busy={remove.isPending}
        disabled={access.disabled}
        disabledReason={access.reason}
        onConfirm={() => {
          if (removing && access.allowed) remove.mutate(removing);
        }}
      />
      <PageBody padded={false}>
        {state ? (
          <QueryState state={state} error={query.error} />
        ) : (
          <div data-connectors className="flex min-w-0 flex-col gap-[12px]">
            {section('source', sources)}
            {section('destination', destinations)}
          </div>
        )}
      </PageBody>
    </>
  );
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as clustersApi from '@/api/clusters';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';

export function CreateClusterDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [advertiseAddr, setAdvertiseAddr] = React.useState('');
  const [tokenSecretRef, setTokenSecretRef] = React.useState('');
  const [tlsVerify, setTlsVerify] = React.useState(true);

  React.useEffect(() => {
    if (!open) {
      setName('');
      setAdvertiseAddr('');
      setTokenSecretRef('');
      setTlsVerify(true);
    }
  }, [open]);

  const create = useMutation({
    mutationFn: () =>
      clustersApi.create({
        name: name.trim(),
        advertise_addr: advertiseAddr.trim(),
        token_secret_ref: tokenSecretRef.trim(),
        tls_verify: tlsVerify,
      }),
    onSuccess: () => {
      toast.success(t('nodes.toast_added'));
      void qc.invalidateQueries({ queryKey: ['clusters'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid =
    name.trim().length === 0 ||
    advertiseAddr.trim().length === 0 ||
    tokenSecretRef.trim().length === 0;
  const controlsDisabled = access.disabled || create.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(value) => !value && onClose()}
      title={t('nodes.drawer_title')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('nodes.submit_label')}
          formId="cluster-form"
        />
      }
    >
      <form
        id="cluster-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (controlsDisabled || invalid) return;
          create.mutate();
        }}
      >
        <FormSection title={t('nodes.section_identity')}>
          <FormRow>
            <FormField label={t('nodes.field_name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('nodes.field_advertise_addr')} required>
              <FormInput
                value={advertiseAddr}
                onChange={(event) => setAdvertiseAddr(event.target.value)}
                placeholder={t('nodes.field_advertise_addr_placeholder')}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
          </FormRow>
        </FormSection>
        <FormSection title={t('nodes.section_auth')}>
          <FormField
            label={t('nodes.field_token_secret')}
            required
            hint={t('nodes.field_token_secret_hint')}
          >
            <FormInput
              value={tokenSecretRef}
              onChange={(event) => setTokenSecretRef(event.target.value)}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('nodes.field_tls_verify')}>
            <label className="flex items-center gap-2 font-sans text-xs text-tx-1">
              <DisabledControl
                disabled={controlsDisabled}
                reason={access.reason}
              >
                <Checkbox
                  checked={tlsVerify}
                  disabled={controlsDisabled}
                  aria-disabled={controlsDisabled || undefined}
                  onCheckedChange={(checked) =>
                    setTlsVerify(checked === true)
                  }
                />
              </DisabledControl>
              <span>{t('nodes.field_tls_verify_inline')}</span>
            </label>
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function OrgMapDrawer({
  cluster,
  access,
  onClose,
}: {
  cluster: clustersApi.RemoteCluster | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const open = cluster !== null;
  const clusterId = cluster?.id ?? '';
  const [localOrg, setLocalOrg] = React.useState('');
  const [remoteOrg, setRemoteOrg] = React.useState('');
  const [tokenRef, setTokenRef] = React.useState('');

  React.useEffect(() => {
    if (!open) {
      setLocalOrg('');
      setRemoteOrg('');
      setTokenRef('');
    }
  }, [open]);

  const query = useQuery({
    queryKey: ['cluster-org-map', clusterId],
    queryFn: () => clustersApi.listOrgMap(clusterId),
    enabled: open,
  });
  const rows = query.data ?? [];

  const save = useMutation({
    mutationFn: () => {
      const payload: clustersApi.OrgMapPayload = {
        local_org_id: localOrg.trim(),
        remote_org_id: remoteOrg.trim(),
      };
      if (tokenRef.trim()) payload.token_secret_ref = tokenRef.trim();
      return clustersApi.putOrgMap(clusterId, payload);
    },
    onSuccess: () => {
      toast.success(t('nodes.org_map.toast_saved'));
      setLocalOrg('');
      setRemoteOrg('');
      setTokenRef('');
      void qc.invalidateQueries({ queryKey: ['cluster-org-map', clusterId] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const remove = useMutation({
    mutationFn: (id: string) => clustersApi.deleteOrgMap(clusterId, id),
    onSuccess: () => {
      toast.success(t('nodes.org_map.toast_removed'));
      void qc.invalidateQueries({ queryKey: ['cluster-org-map', clusterId] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid =
    localOrg.trim().length === 0 || remoteOrg.trim().length === 0;
  const controlsDisabled = access.disabled || save.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(value) => !value && onClose()}
      title={`${t('nodes.org_map.title')}${cluster ? ` — ${cluster.name}` : ''}`}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('nodes.org_map.add')}
          formId="org-map-form"
        />
      }
    >
      <p className="mb-3 font-sans text-xs text-tx-2">
        {t('nodes.org_map.description')}
      </p>
      {rows.length === 0 ? (
        <p className="mb-4 font-sans text-xs text-tx-3">
          {t('nodes.org_map.empty')}
        </p>
      ) : (
        <div className="mb-4 overflow-hidden rounded border border-bd-0">
          <table className="w-full text-left font-sans text-xs">
            <thead className="bg-bg-2 text-tx-2">
              <tr>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.org_map.col_local')}
                </th>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.org_map.col_remote')}
                </th>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.org_map.col_token')}
                </th>
                <th className="w-24" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr
                  key={row.local_org_id}
                  className="border-t border-bd-0"
                >
                  <td className="px-2 py-1.5 text-tx-1">
                    {row.local_org_id}
                  </td>
                  <td className="px-2 py-1.5 text-tx-1">
                    {row.remote_org_id}
                  </td>
                  <td className="px-2 py-1.5">
                    {row.token_secret_ref ? (
                      <Pill tone="green">
                        {t('nodes.org_map.token_set')}
                      </Pill>
                    ) : (
                      <Pill tone="dim">
                        {t('nodes.org_map.token_fallback')}
                      </Pill>
                    )}
                  </td>
                  <td className="px-2 py-1.5 text-right">
                    <ChromeButton
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={access.disabled || remove.isPending}
                      disabledReason={access.reason}
                      onClick={() => remove.mutate(row.local_org_id)}
                      className="enabled:hover:text-red-soft"
                    >
                      {tc('actions.delete')}
                    </ChromeButton>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <form
        id="org-map-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (controlsDisabled || invalid) return;
          save.mutate();
        }}
      >
        <FormSection title={t('nodes.org_map.add')}>
          <FormRow>
            <FormField label={t('nodes.org_map.field_local')} required>
              <FormInput
                value={localOrg}
                onChange={(event) => setLocalOrg(event.target.value)}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('nodes.org_map.field_remote')} required>
              <FormInput
                value={remoteOrg}
                onChange={(event) => setRemoteOrg(event.target.value)}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
          </FormRow>
          <FormField
            label={t('nodes.org_map.field_token')}
            hint={t('nodes.org_map.field_token_hint')}
          >
            <FormInput
              value={tokenRef}
              onChange={(event) => setTokenRef(event.target.value)}
              disabled={controlsDisabled}
              disabledReason={access.reason}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

export function RemoteNodesDrawer({
  cluster,
  onClose,
}: {
  cluster: clustersApi.RemoteCluster | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const open = cluster !== null;
  const clusterId = cluster?.id ?? '';
  const query = useQuery({
    queryKey: ['cluster-nodes', clusterId],
    queryFn: () => clustersApi.listNodes(clusterId),
    enabled: open,
    retry: false,
  });
  const nodes = query.data ?? [];

  return (
    <FormDrawer
      open={open}
      onOpenChange={(value) => !value && onClose()}
      title={`${t('nodes.remote_nodes.title')}${cluster ? ` — ${cluster.name}` : ''}`}
      footer={
        <ChromeButton variant="ghost" onClick={onClose}>
          {tc('actions.close')}
        </ChromeButton>
      }
    >
      {query.isLoading ? (
        <p className="font-sans text-xs text-tx-2">
          {tc('status.loading')}
        </p>
      ) : query.isError ? (
        <p className="font-sans text-xs text-red-soft">
          {t('nodes.remote_nodes.error')}
        </p>
      ) : nodes.length === 0 ? (
        <p className="font-sans text-xs text-tx-3">
          {t('nodes.remote_nodes.empty')}
        </p>
      ) : (
        <div className="overflow-hidden rounded border border-bd-0">
          <table className="w-full text-left font-sans text-xs">
            <thead className="bg-bg-2 text-tx-2">
              <tr>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.remote_nodes.col_node')}
                </th>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.remote_nodes.col_roles')}
                </th>
                <th className="px-2 py-1.5 font-medium">
                  {t('nodes.remote_nodes.col_version')}
                </th>
              </tr>
            </thead>
            <tbody>
              {nodes.map((node) => (
                <tr key={node.node_id} className="border-t border-bd-0">
                  <td className="px-2 py-1.5 text-tx-1">
                    <div className="font-medium">{node.node_id}</div>
                    <div className="text-tx-3">{node.advertise_addr}</div>
                  </td>
                  <td className="px-2 py-1.5">
                    <div className="flex flex-wrap gap-1">
                      {node.roles.map((role) => (
                        <Pill key={role} tone="dim">
                          {role}
                        </Pill>
                      ))}
                    </div>
                  </td>
                  <td className="px-2 py-1.5 text-tx-2">
                    {node.version}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </FormDrawer>
  );
}

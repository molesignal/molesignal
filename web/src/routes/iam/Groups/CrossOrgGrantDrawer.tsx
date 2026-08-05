import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as dashboardsApi from '@/api/dashboards';
import * as grantsApi from '@/api/groups';
import * as streamsApi from '@/api/streams';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import {
  permissionDefinition,
  type PermissionCatalog,
  type PermissionKey,
} from '@/product/permissions';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { useCurrentOrgSelection } from '@/stores/useOrgStore';

import {
  invalidateIamAccess,
  type ResourceType,
  RESOURCE_TYPES,
} from './shared';

export function CrossOrgGrantDrawer({
  access,
  catalog,
  open,
  onClose,
}: {
  access: ActionAccess;
  catalog: PermissionCatalog | undefined;
  open: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const { currentOrgId } = useCurrentOrgSelection();
  const [targetOrgId, setTargetOrgId] = React.useState('');
  const [resourceType, setResourceType] =
    React.useState<ResourceType>('dashboard');
  const [resourceId, setResourceId] = React.useState('');
  const [permissions, setPermissions] = React.useState<PermissionKey[]>([
    'dashboards.read',
  ]);
  const [expiresAt, setExpiresAt] = React.useState('');

  const dashboardsQuery = useQuery({
    queryKey: ['iam', 'grant-dashboards'],
    queryFn: () => dashboardsApi.list(),
    enabled: open && resourceType === 'dashboard',
  });
  const streamsQuery = useQuery({
    queryKey: ['iam', 'grant-streams'],
    queryFn: () => streamsApi.list(),
    enabled: open && resourceType === 'stream',
  });
  const shareTargetsQuery = useQuery({
    queryKey: ['iam', 'share-targets', currentOrgId],
    queryFn: () => grantsApi.listShareTargets(),
    enabled: open,
  });

  React.useEffect(() => {
    if (!open) return;
    setTargetOrgId('');
    setResourceType('dashboard');
    setResourceId('');
    setPermissions(['dashboards.read']);
    setExpiresAt('');
  }, [open]);

  const permissionOptions: PermissionKey[] =
    resourceType === 'dashboard'
      ? ['dashboards.read', 'dashboards.edit']
      : ['streams.read', 'streams.query'];
  const resources =
    resourceType === 'dashboard'
      ? (dashboardsQuery.data ?? []).map((dashboard) => ({
          value: dashboard.id,
          label: dashboard.title,
        }))
      : (streamsQuery.data ?? []).map((stream) => ({
          value: stream.id,
          label: stream.label || stream.name,
        }));
  const save = useMutation({
    mutationFn: () =>
      grantsApi.createCrossOrgGrant({
        target_organization_id: targetOrgId,
        grantee_type: 'organization',
        grantee_id: targetOrgId,
        resource_type: resourceType,
        resource_selector: { ids: [resourceId] },
        permissions,
        conditions: {},
        ...(expiresAt
          ? { expires_at_micros: new Date(expiresAt).getTime() * 1_000 }
          : {}),
      }),
    onSuccess: async () => {
      toast.success(t('groups.cross_org_created'));
      onClose();
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = !targetOrgId || !resourceId || permissions.length === 0;
  const controlsDisabled = access.disabled || save.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('groups.share_cross_org')}
      subtitle={t('groups.cross_org_non_transitive')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('groups.create_share')}
          formId="cross-org-grant-form"
        />
      }
    >
      <form
        id="cross-org-grant-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && !invalid) save.mutate();
        }}
      >
        <FormSection title={t('groups.steps.principal')}>
          <FormField label={t('groups.fields.grantee')}>
            <FormInput
              value={t('groups.principal.target_organization')}
              readOnly
            />
          </FormField>
        </FormSection>
        <FormSection title={t('groups.steps.organization')}>
          <FormField label={t('groups.fields.target_organization')} required>
            <FormSelect
              value={targetOrgId}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={setTargetOrgId}
              options={[
                {
                  value: '',
                  label: t('groups.fields.select_target_organization'),
                },
                ...(shareTargetsQuery.data ?? []).map((organization) => ({
                  value: organization.id,
                  label: organization.name,
                })),
              ]}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('groups.steps.resource')}>
          <FormRow>
            <FormField label={t('groups.fields.resource_type')} required>
              <FormSelect
                value={resourceType}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={(value) => {
                  const next = value as ResourceType;
                  setResourceType(next);
                  setResourceId('');
                  setPermissions(
                    next === 'dashboard'
                      ? ['dashboards.read']
                      : ['streams.read'],
                  );
                }}
                options={RESOURCE_TYPES.map((type) => ({
                  value: type,
                  label: t(`groups.resources.${type}`),
                }))}
              />
            </FormField>
            <FormField label={t('groups.fields.resource')} required>
              <FormSelect
                value={resourceId}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={setResourceId}
                options={[
                  { value: '', label: t('groups.fields.select_resource') },
                  ...resources,
                ]}
              />
            </FormField>
          </FormRow>
        </FormSection>
        <FormSection title={t('groups.steps.permissions')}>
          <FormChecklist<PermissionKey>
            selected={permissions}
            disabled={controlsDisabled}
            disabledReason={access.reason}
            onChange={setPermissions}
            options={permissionOptions.map((permission) => {
              const definition = permissionDefinition(catalog, permission);
              return {
                value: permission,
                label: definition ? t(definition.label_key) : permission,
                ...(definition
                  ? { hint: t(definition.description_key) }
                  : {}),
              };
            })}
          />
        </FormSection>
        <FormSection title={t('groups.steps.constraints')}>
          <FormField label={t('groups.fields.expires_at')}>
            <DateTimePicker
              value={expiresAt}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={setExpiresAt}
            />
          </FormField>
          <p className="rounded-md border border-orange/30 bg-orange-dim px-3 py-2 text-xs text-orange-soft">
            {t('groups.cross_org_non_transitive')}
          </p>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

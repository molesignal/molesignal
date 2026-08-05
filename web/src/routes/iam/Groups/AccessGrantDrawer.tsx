import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as dashboardsApi from '@/api/dashboards';
import * as grantsApi from '@/api/groups';
import * as rolesApi from '@/api/roles';
import * as streamsApi from '@/api/streams';
import * as teamsApi from '@/api/teams';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import type { PermissionCatalog } from '@/product/permissions';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
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
  PermissionPills,
  type PrincipalKind,
  type ResourceType,
  RESOURCE_TYPES,
} from './shared';

export function AccessGrantDrawer({
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
  const { currentOrgId, orgLabel } = useCurrentOrgSelection();
  const [principalKind, setPrincipalKind] =
    React.useState<PrincipalKind>('user');
  const [principalId, setPrincipalId] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [resourceType, setResourceType] =
    React.useState<'all' | ResourceType>('all');
  const [resourceId, setResourceId] = React.useState('');
  const [environment, setEnvironment] = React.useState('');
  const [expiresAt, setExpiresAt] = React.useState('');

  const usersQuery = useQuery({
    queryKey: ['iam', 'grant-users'],
    queryFn: () => usersApi.list(),
    enabled: open,
  });
  const teamsQuery = useQuery({
    queryKey: ['iam', 'grant-teams'],
    queryFn: () => teamsApi.list(),
    enabled: open,
  });
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
    enabled: open,
  });
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

  React.useEffect(() => {
    if (!open) return;
    setPrincipalId('');
    setRoleId('');
    setResourceType('all');
    setResourceId('');
    setEnvironment('');
    setExpiresAt('');
  }, [open]);

  const save = useMutation({
    mutationFn: () =>
      grantsApi.createRoleBinding({
        role_id: roleId,
        principal_type: principalKind,
        principal_id: principalId,
        ...(resourceType === 'all'
          ? {}
          : {
              resource_type: resourceType,
              ...(resourceId ? { resource_id: resourceId } : {}),
            }),
        ...(environment
          ? { conditions: { environment: environment.trim() } }
          : {}),
        ...(expiresAt
          ? { expires_at_micros: new Date(expiresAt).getTime() * 1_000 }
          : {}),
      }),
    onSuccess: async () => {
      toast.success(t('groups.toast_created'));
      onClose();
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const principals =
    principalKind === 'user'
      ? (usersQuery.data ?? []).map((user) => ({
          value: user.id,
          label: user.display_name || user.email,
        }))
      : (teamsQuery.data ?? []).map((team) => ({
          value: team.id,
          label: team.name,
        }));
  const resources =
    resourceType === 'dashboard'
      ? (dashboardsQuery.data ?? []).map((dashboard) => ({
          value: dashboard.id,
          label: dashboard.title,
        }))
      : resourceType === 'stream'
        ? (streamsQuery.data ?? []).map((stream) => ({
            value: stream.id,
            label: stream.label || stream.name,
          }))
        : [];
  const selectedRole = (rolesQuery.data ?? []).find(
    (role) => role.id === roleId,
  );
  const invalid = !principalId || !roleId;
  const controlsDisabled = access.disabled || save.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('groups.new_grant')}
      subtitle={t('groups.drawer_subtitle')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('groups.create_grant')}
          formId="access-grant-form"
        />
      }
    >
      <form
        id="access-grant-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && !invalid) save.mutate();
        }}
      >
        <FormSection title={t('groups.steps.principal')}>
          <FormRow>
            <FormField label={t('groups.fields.principal_type')} required>
              <FormSelect
                value={principalKind}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={(value) => {
                  setPrincipalKind(value as PrincipalKind);
                  setPrincipalId('');
                }}
                options={[
                  { value: 'user', label: t('groups.principal.user') },
                  { value: 'team', label: t('groups.principal.team') },
                ]}
              />
            </FormField>
            <FormField label={t('groups.fields.principal')} required>
              <FormSelect
                value={principalId}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={setPrincipalId}
                options={[
                  { value: '', label: t('groups.fields.select_principal') },
                  ...principals,
                ]}
              />
            </FormField>
          </FormRow>
        </FormSection>

        <FormSection title={t('groups.steps.organization')}>
          <FormField label={t('groups.fields.organization')}>
            <FormInput
              value={`${orgLabel} (${currentOrgId ?? '—'})`}
              readOnly
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
                  setResourceType(value as 'all' | ResourceType);
                  setResourceId('');
                }}
                options={[
                  { value: 'all', label: t('groups.organization_wide') },
                  ...RESOURCE_TYPES.map((type) => ({
                    value: type,
                    label: t(`groups.resources.${type}`),
                  })),
                ]}
              />
            </FormField>
            {resourceType !== 'all' && (
              <FormField label={t('groups.fields.resource')}>
                <FormSelect
                  value={resourceId}
                  disabled={controlsDisabled}
                  disabledReason={access.reason}
                  onChange={setResourceId}
                  options={[
                    {
                      value: '',
                      label: t('groups.fields.all_resources_of_type'),
                    },
                    ...resources,
                  ]}
                />
              </FormField>
            )}
          </FormRow>
        </FormSection>

        <FormSection title={t('groups.steps.permissions')}>
          <FormField label={t('groups.fields.role')} required>
            <FormSelect
              value={roleId}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={setRoleId}
              options={[
                { value: '', label: t('groups.fields.select_role') },
                ...(rolesQuery.data ?? []).map((role) => ({
                  value: role.id,
                  label: role.name,
                })),
              ]}
            />
          </FormField>
          {selectedRole && (
            <PermissionPills
              catalog={catalog}
              permissions={selectedRole.permissions}
            />
          )}
        </FormSection>

        <FormSection title={t('groups.steps.constraints')}>
          <FormRow>
            <FormField
              label={t('groups.fields.environment')}
              hint={t('groups.fields.environment_hint')}
            >
              <FormInput
                value={environment}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={(event) => setEnvironment(event.target.value)}
                placeholder="prod"
              />
            </FormField>
            <FormField label={t('groups.fields.expires_at')}>
              <DateTimePicker
                value={expiresAt}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={setExpiresAt}
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

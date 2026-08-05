import { ArrowRight, Plus, Trash2 } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import { ChromeButton } from '@/shell/chrome';
import {
  FormField,
  FormInput,
  FormSection,
  FormSelect,
} from '@/shell/FormDrawer';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import type { ProviderDraft } from './model';

interface MappingsFormProps {
  draft: ProviderDraft;
  roles: ssoApi.SsoAssignableRole[];
  rolesLoading: boolean;
  rolesError: string | null;
  disabled: boolean;
  onRetryRoles: () => void;
  onChange: (draft: ProviderDraft) => void;
}

const PLATFORM_FIELDS = [
  'subject',
  'email',
  'display_name',
  'groups',
] as const satisfies ReadonlyArray<keyof ssoApi.SsoFieldMapping>;

export function MappingsForm({
  draft,
  roles,
  rolesLoading,
  rolesError,
  disabled,
  onRetryRoles,
  onChange,
}: MappingsFormProps) {
  const { t } = useTranslation('settings-admin');
  const selectedConfig =
    draft.kind === 'oidc'
      ? draft.oidc
      : draft.kind === 'saml'
        ? draft.saml
        : draft.ldap;
  const mapping = selectedConfig.field_mapping;
  const selectedRoleIds = new Set([
    draft.defaultRoleId,
    ...draft.roleMappings.map(({ roleId }) => roleId),
  ]);
  const roleOptions = [
    {
      value: '',
      label: t('sso_providers.drawer.mapping.no_default_role'),
    },
    ...roles.map((role) => ({ value: role.id, label: role.name })),
    ...[...selectedRoleIds]
      .filter(
        (roleId) =>
          roleId &&
          !roles.some((role) => role.id === roleId),
      )
      .map((roleId) => ({ value: roleId, label: roleId })),
  ];
  const rolesUnavailable =
    rolesLoading || rolesError !== null || roles.length === 0;

  const updateField = (
    field: keyof ssoApi.SsoFieldMapping,
    value: string,
  ) => {
    const field_mapping = { ...mapping, [field]: value };
    if (draft.kind === 'oidc') {
      onChange({
        ...draft,
        oidc: { ...draft.oidc, field_mapping },
      });
    } else if (draft.kind === 'saml') {
      onChange({
        ...draft,
        saml: { ...draft.saml, field_mapping },
      });
    } else {
      onChange({
        ...draft,
        ldap: { ...draft.ldap, field_mapping },
      });
    }
  };

  return (
    <>
      <FormSection title={t('sso_providers.drawer.sections.field_mapping')}>
        <div className="mb-3 text-xs leading-relaxed text-tx-2">
          {t(`sso_providers.drawer.mapping.${draft.kind}_hint`)}
        </div>
        <div className="grid grid-cols-[minmax(0,0.8fr)_20px_minmax(0,1.2fr)] items-center gap-x-2 gap-y-2">
          <div className="font-sans text-xs font-semibold uppercase tracking-wide text-tx-3">
            {t('sso_providers.drawer.mapping.platform_field')}
          </div>
          <span aria-hidden />
          <div className="font-sans text-xs font-semibold uppercase tracking-wide text-tx-3">
            {t('sso_providers.drawer.mapping.provider_field')}
          </div>
          {PLATFORM_FIELDS.map((field) => (
            <MappingRow
              key={field}
              label={t(`sso_providers.drawer.mapping.fields.${field}`)}
              value={mapping[field]}
              disabled={disabled}
              onChange={(value) => updateField(field, value)}
            />
          ))}
        </div>
      </FormSection>

      <FormSection title={t('sso_providers.drawer.sections.role_mapping')}>
        {rolesLoading ? (
          <RoleListStatus>
            {t('sso_providers.drawer.mapping.roles_loading')}
          </RoleListStatus>
        ) : rolesError ? (
          <RoleListStatus>
            <span>
              {t('sso_providers.drawer.mapping.roles_error', {
                message: rolesError,
              })}
            </span>
            <ChromeButton
              type="button"
              variant="ghost"
              size="sm"
              disabled={disabled}
              onClick={onRetryRoles}
            >
              {t('sso_providers.drawer.mapping.roles_retry')}
            </ChromeButton>
          </RoleListStatus>
        ) : roles.length === 0 ? (
          <RoleListStatus>
            {t('sso_providers.drawer.mapping.roles_empty')}
          </RoleListStatus>
        ) : null}

        <FormField
          label={t('sso_providers.drawer.mapping.default_role')}
          hint={t('sso_providers.drawer.mapping.default_role_hint')}
        >
          <FormSelect
            value={draft.defaultRoleId}
            onChange={(defaultRoleId) =>
              onChange({ ...draft, defaultRoleId })
            }
            options={roleOptions}
            disabled={disabled || rolesLoading}
          />
        </FormField>

        <div className="mt-3 flex items-center justify-between gap-3">
          <div>
            <div className="font-sans text-xs font-semibold text-tx-0">
              {t('sso_providers.drawer.mapping.group_roles')}
            </div>
            <div className="mt-0.5 text-xs text-tx-2">
              {t('sso_providers.drawer.mapping.group_roles_hint')}
            </div>
          </div>
          <ChromeButton
            type="button"
            variant="ghost"
            size="sm"
            disabled={disabled || rolesUnavailable}
            disabledReason={
              rolesUnavailable
                ? t('sso_providers.drawer.mapping.roles_unavailable')
                : undefined
            }
            onClick={() =>
              onChange({
                ...draft,
                roleMappings: [
                  ...draft.roleMappings,
                  { group: '', roleId: '' },
                ],
              })
            }
          >
            <Plus size={13} aria-hidden />
            {t('sso_providers.drawer.mapping.add_group_role')}
          </ChromeButton>
        </div>

        {draft.roleMappings.length === 0 ? (
          <div className="mt-3 rounded-md bg-bg-2 px-3 py-2.5 text-xs text-tx-3">
            {t('sso_providers.drawer.mapping.no_group_roles')}
          </div>
        ) : (
          <div className="mt-3 flex flex-col gap-2">
            {draft.roleMappings.map((row, index) => (
              <div
                key={index}
                className="grid grid-cols-[minmax(0,1fr)_20px_minmax(0,1fr)_32px] items-center gap-2"
              >
                <FormInput
                  value={row.group}
                  onChange={(event) => {
                    const roleMappings = [...draft.roleMappings];
                    roleMappings[index] = {
                      ...row,
                      group: event.target.value,
                    };
                    onChange({ ...draft, roleMappings });
                  }}
                  placeholder={t(
                    'sso_providers.drawer.mapping.group_value_placeholder',
                  )}
                  aria-label={t(
                    'sso_providers.drawer.mapping.group_value',
                  )}
                />
                <ArrowRight
                  size={13}
                  className="text-tx-3"
                  aria-hidden
                />
                <FormSelect
                  value={row.roleId}
                  onChange={(roleId) => {
                    const roleMappings = [...draft.roleMappings];
                    roleMappings[index] = { ...row, roleId };
                    onChange({ ...draft, roleMappings });
                  }}
                  options={roleOptions.slice(1)}
                  placeholder={t(
                    'sso_providers.drawer.mapping.select_role',
                  )}
                  ariaLabel={t(
                    'sso_providers.drawer.mapping.selected_role',
                  )}
                  disabled={disabled || rolesUnavailable}
                  disabledReason={
                    rolesUnavailable
                      ? t('sso_providers.drawer.mapping.roles_unavailable')
                      : undefined
                  }
                />
                <Tooltip>
                  <TooltipTrigger asChild>
                    <ChromeButton
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 w-8 px-0 text-tx-2 hover:text-red-soft"
                      disabled={disabled}
                      aria-label={t(
                        'sso_providers.drawer.mapping.remove_group_role',
                      )}
                      onClick={() =>
                        onChange({
                          ...draft,
                          roleMappings: draft.roleMappings.filter(
                            (_, rowIndex) => rowIndex !== index,
                          ),
                        })
                      }
                    >
                      <Trash2 size={13} aria-hidden />
                    </ChromeButton>
                  </TooltipTrigger>
                  <TooltipContent>
                    {t('sso_providers.drawer.mapping.remove_group_role')}
                  </TooltipContent>
                </Tooltip>
              </div>
            ))}
          </div>
        )}
      </FormSection>
    </>
  );
}

function RoleListStatus({ children }: { children: ReactNode }) {
  return (
    <div
      role="status"
      className="flex min-h-9 items-center justify-between gap-3 rounded-md bg-bg-2 px-3 py-2 text-xs text-tx-2"
    >
      {children}
    </div>
  );
}

function MappingRow({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <>
      <div className="rounded-md bg-bg-2 px-3 py-2 font-sans text-xs font-semibold text-tx-1">
        {label}
      </div>
      <ArrowRight size={13} className="text-tx-3" aria-hidden />
      <FormInput
        value={value}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        aria-label={label}
        className="font-mono"
      />
    </>
  );
}

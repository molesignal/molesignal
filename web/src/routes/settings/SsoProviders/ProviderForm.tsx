import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSection, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';

import { acceptsPublicAccounts } from './access';
import { MappingsForm } from './MappingsForm';
import { type ProviderDraft, type SsoCallbackUrls, switchKind } from './model';
import { platformKinds, presetFor, templatePresetFor } from './presets';
import { AccessSection } from './sections/AccessSection';
import { LdapSection } from './sections/LdapSection';
import { Oauth2Section } from './sections/Oauth2Section';
import { OidcSection } from './sections/OidcSection';
import { SamlSection } from './sections/SamlSection';

interface ProviderFormProps {
  draft: ProviderDraft;
  /** A new provider can still go back to the platform picker. */
  creating: boolean;
  /** A saved provider's test sign-in. */
  testRow?: React.ReactNode;
  callbackUrls: SsoCallbackUrls;
  roles: ssoApi.SsoAssignableRole[];
  rolesLoading: boolean;
  rolesError: string | null;
  /** The organization's email domains; undefined while they load. */
  emailDomains: string[] | undefined;
  disabled: boolean;
  onRetryRoles: () => void;
  onChange: (draft: ProviderDraft) => void;
  onChangePlatform: () => void;
}

export function ProviderForm({
  draft,
  creating,
  testRow,
  callbackUrls,
  roles,
  rolesLoading,
  rolesError,
  emailDomains,
  disabled,
  onRetryRoles,
  onChange,
  onChangePlatform,
}: ProviderFormProps) {
  const { t } = useTranslation('settings-admin');
  const preset = presetFor(draft.platform, draft.kind);
  const template = draft.presetInputs ? templatePresetFor(draft.platform, draft.kind) : null;
  const kinds = platformKinds(draft.platform);
  const generic = draft.platform === 'custom';
  const platformName = generic
    ? t(`sso_providers.kinds.${draft.kind}`)
    : t(`sso_providers.platforms.${draft.platform}`);
  const sectionTitle = t(`sso_providers.drawer.sections.${draft.kind}`);

  return (
    <fieldset
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className="m-0 min-w-0 border-0 p-0"
    >
      <div className="mb-6 flex items-center gap-3" data-testid="sso-provider-platform">
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-bg-2">
          <SsoPlatformMark platform={draft.platform} kind={draft.kind} size={20} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate font-sans text-sm font-strong text-tx-0">
            {platformName}
          </span>
          <span className="block truncate font-sans text-xs text-tx-3">
            {generic ? t('sso_providers.picker.generic') : t(`sso_providers.kinds.${draft.kind}`)}
          </span>
        </span>
        {creating && (
          <ChromeButton type="button" size="sm" onClick={onChangePlatform}>
            {t('sso_providers.picker.change')}
          </ChromeButton>
        )}
      </div>

      {testRow}

      <FormSection title={t('sso_providers.drawer.sections.identity')}>
        <FormRow className={cn(kinds.length < 2 && 'grid-cols-1')}>
          <FormField label={t('sso_providers.drawer.fields.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) => onChange({ ...draft, name: event.target.value })}
              required
            />
          </FormField>
          {kinds.length > 1 && (
            <FormField label={t('sso_providers.drawer.fields.protocol')} required>
              <FormSelect
                value={draft.kind}
                onChange={(kind) =>
                  onChange(switchKind(draft, kind as ssoApi.SsoProviderKind, callbackUrls))
                }
                options={kinds.map((kind) => ({
                  value: kind,
                  label: t(`sso_providers.kinds.${kind}`),
                }))}
                ariaLabel={t('sso_providers.drawer.fields.protocol')}
              />
            </FormField>
          )}
        </FormRow>
      </FormSection>

      {draft.kind === 'oidc' && (
        <OidcSection
          draft={draft}
          preset={template?.kind === 'oidc' ? template : null}
          title={sectionTitle}
          onChange={onChange}
        />
      )}
      {draft.kind === 'oauth2' && (
        <Oauth2Section
          draft={draft}
          preset={template?.kind === 'oauth2' ? template : null}
          title={sectionTitle}
          onChange={onChange}
        />
      )}
      {draft.kind === 'saml' && (
        <SamlSection
          draft={draft}
          preset={preset?.kind === 'saml' ? preset : null}
          title={sectionTitle}
          onChange={onChange}
        />
      )}
      {draft.kind === 'ldap' && (
        <LdapSection
          draft={draft}
          preset={preset?.kind === 'ldap' ? preset : null}
          title={sectionTitle}
          onChange={onChange}
        />
      )}

      <MappingsForm
        draft={draft}
        preset={preset}
        roles={roles}
        rolesLoading={rolesLoading}
        rolesError={rolesError}
        disabled={disabled}
        onRetryRoles={onRetryRoles}
        onChange={onChange}
      />

      <AccessSection
        draft={draft}
        groupsAvailable={preset?.groups ?? true}
        publicAccounts={acceptsPublicAccounts(draft.kind, draft)}
        emailDomains={emailDomains}
        onChange={onChange}
      />
    </fieldset>
  );
}

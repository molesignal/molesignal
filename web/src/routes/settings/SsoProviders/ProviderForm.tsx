import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import {
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormTextarea,
} from '@/shell/FormDrawer';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { Switch } from '@/shell/ui/switch';

import { CallbackUrlField } from './CallbackUrlField';
import { MappingsForm } from './MappingsForm';
import type { ProviderDraft } from './model';

interface ProviderFormProps {
  draft: ProviderDraft;
  roles: ssoApi.SsoAssignableRole[];
  rolesLoading: boolean;
  rolesError: string | null;
  disabled: boolean;
  onRetryRoles: () => void;
  onChange: (draft: ProviderDraft) => void;
}

export function ProviderForm({
  draft,
  roles,
  rolesLoading,
  rolesError,
  disabled,
  onRetryRoles,
  onChange,
}: ProviderFormProps) {
  const { t } = useTranslation('settings-admin');
  const setOidc = (patch: Partial<ProviderDraft['oidc']>) =>
    onChange({ ...draft, oidc: { ...draft.oidc, ...patch } });
  const setSaml = (patch: Partial<ProviderDraft['saml']>) =>
    onChange({ ...draft, saml: { ...draft.saml, ...patch } });
  const setLdap = (patch: Partial<ProviderDraft['ldap']>) =>
    onChange({ ...draft, ldap: { ...draft.ldap, ...patch } });

  return (
    <fieldset
      disabled={disabled}
      aria-disabled={disabled || undefined}
      className="m-0 min-w-0 border-0 p-0"
    >
      <FormSection title={t('sso_providers.drawer.sections.identity')}>
        <FormRow>
          <FormField label={t('sso_providers.drawer.fields.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) =>
                onChange({ ...draft, name: event.target.value })
              }
              required
            />
          </FormField>
          <FormField label={t('sso_providers.drawer.fields.kind')} required>
            <Select
              value={draft.kind}
              onValueChange={(kind) =>
                onChange({
                  ...draft,
                  kind: kind as ProviderDraft['kind'],
                })
              }
            >
              <SelectTrigger className="h-8 rounded-md border-bd-1 bg-bg-2 px-2.5 font-sans text-xs text-tx-0">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="oidc">
                  {t('sso_providers.kinds.oidc')}
                </SelectItem>
                <SelectItem value="saml">
                  {t('sso_providers.kinds.saml')}
                </SelectItem>
                <SelectItem value="ldap">
                  {t('sso_providers.kinds.ldap')}
                </SelectItem>
              </SelectContent>
            </Select>
          </FormField>
        </FormRow>
      </FormSection>

      {draft.kind === 'oidc' && (
        <FormSection title={t('sso_providers.drawer.sections.oidc')}>
          <FormField label={t('sso_providers.drawer.fields.issuer')} required>
            <FormInput
              value={draft.oidc.issuer}
              onChange={(event) => setOidc({ issuer: event.target.value })}
              placeholder="https://idp.example.com"
              required
            />
          </FormField>
          <FormRow>
            <FormField
              label={t('sso_providers.drawer.fields.authorize_url')}
              required
            >
              <FormInput
                value={draft.oidc.authorize_url}
                onChange={(event) =>
                  setOidc({ authorize_url: event.target.value })
                }
                placeholder="https://idp.example.com/oauth2/authorize"
                required
              />
            </FormField>
            <FormField
              label={t('sso_providers.drawer.fields.token_url')}
              required
            >
              <FormInput
                value={draft.oidc.token_url}
                onChange={(event) =>
                  setOidc({ token_url: event.target.value })
                }
                placeholder="https://idp.example.com/oauth2/token"
                required
              />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField
              label={t('sso_providers.drawer.fields.client_id')}
              required
            >
              <FormInput
                value={draft.oidc.client_id}
                onChange={(event) =>
                  setOidc({ client_id: event.target.value })
                }
                required
              />
            </FormField>
            <FormField
              label={t('sso_providers.drawer.fields.client_secret')}
              {...(draft.oidc.has_client_secret
                ? {
                    hint: t(
                      'sso_providers.drawer.fields.secret_preserved_hint',
                    ),
                  }
                : {})}
            >
              <FormInput
                type="password"
                value={draft.oidc.client_secret ?? ''}
                onChange={(event) =>
                  setOidc({ client_secret: event.target.value })
                }
                autoComplete="new-password"
              />
            </FormField>
          </FormRow>
          <CallbackUrlField
            label={t('sso_providers.drawer.fields.redirect_uri')}
            hint={t('sso_providers.drawer.fields.oidc_redirect_uri_hint')}
            value={draft.oidc.redirect_uri}
            copyLabel={t('sso_providers.drawer.fields.copy_redirect_uri')}
            copiedLabel={t('sso_providers.drawer.fields.redirect_uri_copied')}
          />
          <FormField
            label={t('sso_providers.drawer.fields.scopes')}
            hint={t('sso_providers.drawer.fields.scopes_hint')}
          >
            <FormInput
              value={draft.oidc.scopes.join(' ')}
              onChange={(event) =>
                setOidc({
                  scopes: event.target.value
                    .split(/\s+/)
                    .filter(Boolean),
                })
              }
            />
          </FormField>
          <FormRow>
            <FormField
              label={t('sso_providers.drawer.fields.userinfo_url')}
            >
              <FormInput
                value={draft.oidc.userinfo_url ?? ''}
                onChange={(event) =>
                  setOidc({ userinfo_url: event.target.value })
                }
              />
            </FormField>
            <FormField label={t('sso_providers.drawer.fields.jwks_uri')}>
              <FormInput
                value={draft.oidc.jwks_uri ?? ''}
                onChange={(event) =>
                  setOidc({ jwks_uri: event.target.value })
                }
              />
            </FormField>
          </FormRow>
        </FormSection>
      )}

      {draft.kind === 'saml' && (
        <FormSection title={t('sso_providers.drawer.sections.saml')}>
          <FormRow>
            <FormField
              label={t('sso_providers.drawer.fields.sp_entity_id')}
              required
            >
              <FormInput
                value={draft.saml.sp_entity_id}
                onChange={(event) =>
                  setSaml({ sp_entity_id: event.target.value })
                }
                required
              />
            </FormField>
            <FormField
              label={t('sso_providers.drawer.fields.idp_entity_id')}
              required
            >
              <FormInput
                value={draft.saml.idp_entity_id}
                onChange={(event) =>
                  setSaml({ idp_entity_id: event.target.value })
                }
                required
              />
            </FormField>
          </FormRow>
          <FormField
            label={t('sso_providers.drawer.fields.idp_sso_url')}
            required
          >
            <FormInput
              value={draft.saml.idp_sso_url}
              onChange={(event) =>
                setSaml({ idp_sso_url: event.target.value })
              }
              required
            />
          </FormField>
          <CallbackUrlField
            label={t('sso_providers.drawer.fields.assertion_consumer_url')}
            hint={t('sso_providers.drawer.fields.saml_callback_url_hint')}
            value={draft.saml.assertion_consumer_url}
            copyLabel={t('sso_providers.drawer.fields.copy_callback_url')}
            copiedLabel={t('sso_providers.drawer.fields.callback_url_copied')}
          />
          <FormField
            label={t('sso_providers.drawer.fields.idp_x509_cert')}
            required
          >
            <FormTextarea
              rows={6}
              value={draft.saml.idp_x509_cert}
              onChange={(event) =>
                setSaml({ idp_x509_cert: event.target.value })
              }
              placeholder={t(
                'sso_providers.drawer.fields.idp_x509_cert_placeholder',
              )}
              className="font-mono"
              required
            />
          </FormField>
        </FormSection>
      )}

      {draft.kind === 'ldap' && (
        <FormSection title={t('sso_providers.drawer.sections.ldap')}>
          <FormRow>
            <FormField
              label={t('sso_providers.drawer.fields.ldap_url')}
              hint={t('sso_providers.drawer.fields.ldap_url_hint')}
              required
            >
              <FormInput
                value={draft.ldap.url}
                onChange={(event) =>
                  setLdap({ url: event.target.value })
                }
                placeholder="ldaps://ldap.example.com:636"
                required
              />
            </FormField>
            <FormField
              label={t('sso_providers.drawer.fields.start_tls')}
              hint={t('sso_providers.drawer.fields.start_tls_hint')}
            >
              <div className="flex h-8 items-center gap-2.5">
                <Switch
                  checked={draft.ldap.start_tls}
                  onCheckedChange={(start_tls) => setLdap({ start_tls })}
                  aria-label={t('sso_providers.drawer.fields.start_tls')}
                />
                <span className="font-sans text-xs text-tx-1">
                  {draft.ldap.start_tls
                    ? t('sso_providers.drawer.fields.start_tls_on')
                    : t('sso_providers.drawer.fields.start_tls_off')}
                </span>
              </div>
            </FormField>
          </FormRow>
          <FormField
            label={t('sso_providers.drawer.fields.base_dn')}
            required
          >
            <FormInput
              value={draft.ldap.base_dn}
              onChange={(event) =>
                setLdap({ base_dn: event.target.value })
              }
              placeholder={t(
                'sso_providers.drawer.fields.base_dn_placeholder',
              )}
              required
            />
          </FormField>
          <FormField
            label={t('sso_providers.drawer.fields.user_filter')}
            hint={t('sso_providers.drawer.fields.user_filter_hint')}
            required
          >
            <FormInput
              value={draft.ldap.user_filter}
              onChange={(event) =>
                setLdap({ user_filter: event.target.value })
              }
              className="font-mono"
              required
            />
          </FormField>
          <FormRow>
            <FormField label={t('sso_providers.drawer.fields.bind_dn')}>
              <FormInput
                value={draft.ldap.bind_dn}
                onChange={(event) =>
                  setLdap({ bind_dn: event.target.value })
                }
                placeholder={t(
                  'sso_providers.drawer.fields.bind_dn_placeholder',
                )}
              />
            </FormField>
            <FormField
              label={t('sso_providers.drawer.fields.bind_password')}
              hint={
                draft.ldap.has_bind_password
                  ? t('sso_providers.drawer.fields.secret_preserved_hint')
                  : t('sso_providers.drawer.fields.bind_pair_hint')
              }
            >
              <FormInput
                type="password"
                value={draft.ldap.bind_password ?? ''}
                onChange={(event) =>
                  setLdap({ bind_password: event.target.value })
                }
                autoComplete="new-password"
              />
            </FormField>
          </FormRow>
        </FormSection>
      )}

      <MappingsForm
        draft={draft}
        roles={roles}
        rolesLoading={rolesLoading}
        rolesError={rolesError}
        disabled={disabled}
        onRetryRoles={onRetryRoles}
        onChange={onChange}
      />
    </fieldset>
  );
}

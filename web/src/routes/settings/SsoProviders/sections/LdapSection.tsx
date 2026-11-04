import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import { FormField, FormInput, FormRow, FormSection } from '@/shell/FormDrawer';
import { Switch } from '@/shell/ui/switch';

import type { ProviderDraft } from '../model';
import type { LdapPreset } from '../presets';

export function LdapSection({
  draft,
  preset,
  title,
  onChange,
}: {
  draft: ProviderDraft;
  preset: LdapPreset | null;
  title: string;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const config = draft.ldap;
  const setLdap = (patch: Partial<ssoApi.LdapProviderConfig>) =>
    onChange({ ...draft, ldap: { ...config, ...patch } });
  const field = (key: string) => t(`sso_providers.drawer.fields.${key}`);

  return (
    <FormSection
      title={title}
      {...(preset ? { description: t(`sso_providers.presets.hints.${preset.hint}`) } : {})}
    >
      <FormRow>
        <FormField label={field('ldap_url')} hint={field('ldap_url_hint')} required>
          <FormInput
            value={config.url}
            onChange={(event) => setLdap({ url: event.target.value })}
            placeholder={preset?.placeholders.url ?? 'ldaps://ldap.example.com:636'}
            required
          />
        </FormField>
        <FormField label={field('start_tls')} hint={field('start_tls_hint')}>
          <div className="flex h-8 items-center gap-2.5">
            <Switch
              checked={config.startTls}
              onCheckedChange={(start_tls) => setLdap({ startTls: start_tls })}
              aria-label={field('start_tls')}
            />
            <span className="font-sans text-xs text-tx-1">
              {config.startTls ? field('start_tls_on') : field('start_tls_off')}
            </span>
          </div>
        </FormField>
      </FormRow>
      <FormField label={field('base_dn')} required>
        <FormInput
          value={config.baseDn}
          onChange={(event) => setLdap({ baseDn: event.target.value })}
          placeholder={preset?.placeholders.baseDn ?? field('base_dn_placeholder')}
          required
        />
      </FormField>
      <FormField label={field('user_filter')} hint={field('user_filter_hint')} required>
        <FormInput
          value={config.userFilter}
          onChange={(event) => setLdap({ userFilter: event.target.value })}
          className="font-mono"
          spellCheck={false}
          required
        />
      </FormField>
      <FormRow>
        <FormField label={field('bind_dn')}>
          <FormInput
            value={config.bindDn}
            onChange={(event) => setLdap({ bindDn: event.target.value })}
            placeholder={preset?.placeholders.bindDn ?? field('bind_dn_placeholder')}
          />
        </FormField>
        <FormField
          label={field('bind_password')}
          hint={config.hasBindPassword ? field('secret_preserved_hint') : field('bind_pair_hint')}
        >
          <FormInput
            type="password"
            value={config.bindPassword ?? ''}
            onChange={(event) => setLdap({ bindPassword: event.target.value })}
            autoComplete="new-password"
          />
        </FormField>
      </FormRow>
    </FormSection>
  );
}

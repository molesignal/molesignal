import { useTranslation } from 'react-i18next';

import { FormField, FormInput, FormRow } from '@/shell/FormDrawer';

import { CallbackUrlField } from '../CallbackUrlField';

interface ClientConfig {
  clientId: string;
  clientSecret?: string;
  hasClientSecret?: boolean;
  redirectUri: string;
}

/** The client MoleSignal is registered as at the platform, and the address it calls back. */
export function ClientFields({
  config,
  secretRequired,
  presetHint,
  onChange,
}: {
  config: ClientConfig;
  /** OAuth 2.0 apps are confidential; a saved secret counts. */
  secretRequired?: boolean;
  /** The platform's own hint already says where the redirect URI goes. */
  presetHint?: boolean;
  onChange: (patch: Partial<ClientConfig>) => void;
}) {
  const { t } = useTranslation('settings-admin');
  return (
    <>
      <CallbackUrlField
        label={t('sso_providers.drawer.fields.redirect_uri')}
        {...(presetHint ? {} : { hint: t('sso_providers.drawer.fields.oidc_redirect_uri_hint') })}
        value={config.redirectUri}
        copyLabel={t('sso_providers.drawer.fields.copy_redirect_uri')}
        copiedLabel={t('sso_providers.drawer.fields.redirect_uri_copied')}
      />
      <FormRow>
        <FormField label={t('sso_providers.drawer.fields.client_id')} required>
          <FormInput
            value={config.clientId}
            onChange={(event) => onChange({ clientId: event.target.value })}
            spellCheck={false}
            required
          />
        </FormField>
        <FormField
          label={t('sso_providers.drawer.fields.client_secret')}
          required={Boolean(secretRequired && !config.hasClientSecret)}
          {...(config.hasClientSecret
            ? { hint: t('sso_providers.drawer.fields.secret_preserved_hint') }
            : {})}
        >
          <FormInput
            type="password"
            value={config.clientSecret ?? ''}
            onChange={(event) => onChange({ clientSecret: event.target.value })}
            autoComplete="new-password"
          />
        </FormField>
      </FormRow>
    </>
  );
}

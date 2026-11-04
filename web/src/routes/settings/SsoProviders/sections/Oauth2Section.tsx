import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import { FormField, FormInput, FormRow, FormSection } from '@/shell/FormDrawer';

import { setPresetInputs, type ProviderDraft } from '../model';
import type { Oauth2Preset } from '../presets';
import { ClientFields } from './ClientFields';
import { EndpointList, ScopesField } from './EndpointList';
import { PresetInputs } from './PresetInputs';

/** OAuth 2.0 without ID tokens: the user comes from the user endpoint. */
export function Oauth2Section({
  draft,
  preset,
  title,
  onChange,
}: {
  draft: ProviderDraft;
  preset: Oauth2Preset | null;
  title: string;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const config = draft.oauth2;
  const setOauth2 = (patch: Partial<ssoApi.Oauth2ProviderConfig>) =>
    onChange({ ...draft, oauth2: { ...config, ...patch } });
  const field = (key: string) => t(`sso_providers.drawer.fields.${key}`);

  if (preset && draft.presetInputs) {
    return (
      <FormSection title={title} description={t(`sso_providers.presets.hints.${preset.hint}`)}>
        <PresetInputs
          preset={preset}
          values={draft.presetInputs}
          onChange={(inputs) => onChange(setPresetInputs(draft, inputs))}
        />
        <ClientFields config={config} secretRequired presetHint onChange={setOauth2} />
        <EndpointList
          endpoints={[
            { label: field('authorize_url'), value: config.authorizeUrl },
            { label: field('token_url'), value: config.tokenUrl },
            { label: field('userinfo_url'), value: config.userinfoUrl },
          ]}
          scopes={config.scopes}
          onScopesChange={(scopes) => setOauth2({ scopes })}
        />
      </FormSection>
    );
  }

  return (
    <FormSection title={title}>
      <FormRow>
        <FormField label={field('authorize_url')} required>
          <FormInput
            value={config.authorizeUrl}
            onChange={(event) => setOauth2({ authorizeUrl: event.target.value })}
            placeholder="https://idp.example.com/oauth/authorize"
            required
          />
        </FormField>
        <FormField label={field('token_url')} required>
          <FormInput
            value={config.tokenUrl}
            onChange={(event) => setOauth2({ tokenUrl: event.target.value })}
            placeholder="https://idp.example.com/oauth/token"
            required
          />
        </FormField>
      </FormRow>
      <FormField
        label={field('userinfo_url')}
        hint={field('oauth2_userinfo_hint')}
        required
      >
        <FormInput
          value={config.userinfoUrl}
          onChange={(event) => setOauth2({ userinfoUrl: event.target.value })}
          placeholder="https://idp.example.com/api/user"
          required
        />
      </FormField>
      <ClientFields config={config} secretRequired onChange={setOauth2} />
      <ScopesField scopes={config.scopes} onChange={(scopes) => setOauth2({ scopes })} />
    </FormSection>
  );
}

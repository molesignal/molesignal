import { useMutation } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSection } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { setPresetInputs, type ProviderDraft } from '../model';
import type { OidcPreset } from '../presets';
import { ClientFields } from './ClientFields';
import { EndpointList, ScopesField } from './EndpointList';
import { PresetInputs } from './PresetInputs';

export function OidcSection({
  draft,
  preset,
  title,
  onChange,
}: {
  draft: ProviderDraft;
  /** Set while the endpoints follow the preset's inputs. */
  preset: OidcPreset | null;
  title: string;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const config = draft.oidc;
  const setOidc = (patch: Partial<ssoApi.OidcProviderConfig>) =>
    onChange({ ...draft, oidc: { ...config, ...patch } });
  const field = (key: string) => t(`sso_providers.drawer.fields.${key}`);

  if (preset && draft.presetInputs) {
    return (
      <FormSection title={title} description={t(`sso_providers.presets.hints.${preset.hint}`)}>
        <PresetInputs
          preset={preset}
          values={draft.presetInputs}
          onChange={(inputs) => onChange(setPresetInputs(draft, inputs))}
        />
        <ClientFields config={config} presetHint onChange={setOidc} />
        <EndpointList
          endpoints={[
            { label: field('issuer'), value: config.issuer },
            { label: field('authorize_url'), value: config.authorizeUrl },
            { label: field('token_url'), value: config.tokenUrl },
            { label: field('userinfo_url'), value: config.userinfoUrl ?? '' },
            { label: field('jwks_uri'), value: config.jwksUri ?? '' },
          ]}
          scopes={config.scopes}
          onScopesChange={(scopes) => setOidc({ scopes })}
        />
      </FormSection>
    );
  }

  return (
    <FormSection title={title}>
      <IssuerField draft={draft} onChange={onChange} />
      <FormRow>
        <FormField label={field('authorize_url')} required>
          <FormInput
            value={config.authorizeUrl}
            onChange={(event) => setOidc({ authorizeUrl: event.target.value })}
            placeholder="https://idp.example.com/oauth2/authorize"
            required
          />
        </FormField>
        <FormField label={field('token_url')} required>
          <FormInput
            value={config.tokenUrl}
            onChange={(event) => setOidc({ tokenUrl: event.target.value })}
            placeholder="https://idp.example.com/oauth2/token"
            required
          />
        </FormField>
      </FormRow>
      <FormRow>
        <FormField label={field('userinfo_url')}>
          <FormInput
            value={config.userinfoUrl ?? ''}
            onChange={(event) => setOidc({ userinfoUrl: event.target.value })}
          />
        </FormField>
        <FormField label={field('jwks_uri')}>
          <FormInput
            value={config.jwksUri ?? ''}
            onChange={(event) => setOidc({ jwksUri: event.target.value })}
          />
        </FormField>
      </FormRow>
      <ClientFields config={config} onChange={setOidc} />
      <ScopesField scopes={config.scopes} onChange={(scopes) => setOidc({ scopes })} />
    </FormSection>
  );
}

/** The issuer, and reading its discovery document to fill the endpoints. */
function IssuerField({
  draft,
  onChange,
}: {
  draft: ProviderDraft;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const issuer = draft.oidc.issuer;
  const discovery = useMutation({
    mutationFn: () => ssoApi.discover(issuer),
    onSuccess: (found) => {
      onChange({
        ...draft,
        oidc: {
          ...draft.oidc,
          issuer: found.issuer,
          authorizeUrl: found.authorizeUrl,
          tokenUrl: found.tokenUrl,
          userinfoUrl: found.userinfoUrl ?? '',
          jwksUri: found.jwksUri ?? '',
        },
      });
      toast.success(t('sso_providers.drawer.fields.discovered'));
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  return (
    <FormField
      label={t('sso_providers.drawer.fields.issuer')}
      hint={t('sso_providers.drawer.fields.discover_hint')}
      required
    >
      <div className="flex min-w-0 gap-2">
        <FormInput
          value={issuer}
          onChange={(event) =>
            onChange({ ...draft, oidc: { ...draft.oidc, issuer: event.target.value } })
          }
          placeholder="https://idp.example.com"
          required
        />
        <ChromeButton
          type="button"
          className="h-9 shrink-0"
          disabled={!issuer.trim() || discovery.isPending}
          onClick={() => discovery.mutate()}
        >
          {discovery.isPending
            ? t('sso_providers.drawer.fields.discovering')
            : t('sso_providers.drawer.fields.discover')}
        </ChromeButton>
      </div>
    </FormField>
  );
}

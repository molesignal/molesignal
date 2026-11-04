import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormRow, FormSection, FormTextarea } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { CallbackUrlField } from '../CallbackUrlField';
import type { ProviderDraft } from '../model';
import type { SamlPreset } from '../presets';

export function SamlSection({
  draft,
  preset,
  title,
  onChange,
}: {
  draft: ProviderDraft;
  preset: SamlPreset | null;
  title: string;
  onChange: (draft: ProviderDraft) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const config = draft.saml;
  const setSaml = (patch: Partial<ssoApi.SamlProviderConfig>) =>
    onChange({ ...draft, saml: { ...config, ...patch } });
  const field = (key: string) => t(`sso_providers.drawer.fields.${key}`);

  return (
    <FormSection
      title={title}
      {...(preset ? { description: t(`sso_providers.presets.hints.${preset.hint}`) } : {})}
    >
      <CallbackUrlField
        label={field('assertion_consumer_url')}
        {...(preset ? {} : { hint: field('saml_callback_url_hint') })}
        value={config.assertionConsumerUrl}
        copyLabel={field('copy_callback_url')}
        copiedLabel={field('callback_url_copied')}
      />
      <FormField label={field('sp_entity_id')} hint={field('sp_entity_id_hint')} required>
        <FormInput
          value={config.spEntityId}
          onChange={(event) => setSaml({ spEntityId: event.target.value })}
          required
        />
      </FormField>
      <MetadataImport
        startWithXml={preset?.metadataPlaceholder === 'xml'}
        urlPlaceholder={
          preset && preset.metadataPlaceholder !== 'xml'
            ? t(`sso_providers.presets.metadata.${preset.metadataPlaceholder}`)
            : 'https://idp.example.com/saml/metadata'
        }
        onImported={(metadata) => setSaml(metadata)}
      />
      <FormRow>
        <FormField label={field('idp_entity_id')} required>
          <FormInput
            value={config.idpEntityId}
            onChange={(event) => setSaml({ idpEntityId: event.target.value })}
            required
          />
        </FormField>
        <FormField label={field('idp_sso_url')} required>
          <FormInput
            value={config.idpSsoUrl}
            onChange={(event) => setSaml({ idpSsoUrl: event.target.value })}
            required
          />
        </FormField>
      </FormRow>
      <FormField label={field('idp_x509_cert')} required>
        <FormTextarea
          rows={6}
          value={config.idpX509Cert}
          onChange={(event) => setSaml({ idpX509Cert: event.target.value })}
          placeholder={field('idp_x509_cert_placeholder')}
          className="font-mono"
          required
        />
      </FormField>
    </FormSection>
  );
}

/** Fills the IdP entity ID, sign-on URL and signing certificate from the IdP's metadata. */
function MetadataImport({
  startWithXml,
  urlPlaceholder,
  onImported,
}: {
  startWithXml: boolean;
  urlPlaceholder: string;
  onImported: (metadata: ssoApi.SamlIdpMetadata) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const [useXml, setUseXml] = React.useState(startWithXml);
  const [url, setUrl] = React.useState('');
  const [xml, setXml] = React.useState('');
  const source = useXml ? xml.trim() : url.trim();
  const importing = useMutation({
    mutationFn: () => ssoApi.readSamlMetadata(useXml ? { xml: source } : { url: source }),
    onSuccess: (metadata) => {
      onImported(metadata);
      toast.success(t('sso_providers.drawer.metadata.imported'));
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const importButton = (
    <ChromeButton
      type="button"
      className="h-9 shrink-0"
      disabled={!source || importing.isPending}
      onClick={() => importing.mutate()}
    >
      {importing.isPending
        ? t('sso_providers.drawer.metadata.importing')
        : t('sso_providers.drawer.metadata.import')}
    </ChromeButton>
  );
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex items-center justify-between gap-3">
        <span className={uiLabelClass}>{t('sso_providers.drawer.metadata.label')}</span>
        <button
          type="button"
          className="cursor-pointer font-sans text-xs text-[var(--blue-ink)] hover:underline"
          onClick={() => setUseXml((value) => !value)}
        >
          {useXml
            ? t('sso_providers.drawer.metadata.use_url')
            : t('sso_providers.drawer.metadata.use_xml')}
        </button>
      </div>
      {useXml ? (
        <>
          <FormTextarea
            rows={4}
            value={xml}
            onChange={(event) => setXml(event.target.value)}
            placeholder={t('sso_providers.presets.metadata.xml')}
            aria-label={t('sso_providers.drawer.metadata.xml')}
            className="font-mono"
            spellCheck={false}
          />
          <div className="flex justify-end">{importButton}</div>
        </>
      ) : (
        <div className="flex min-w-0 gap-2">
          <FormInput
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            placeholder={urlPlaceholder}
            aria-label={t('sso_providers.drawer.metadata.url')}
            spellCheck={false}
          />
          {importButton}
        </div>
      )}
      <span className="text-xs leading-relaxed text-tx-3">
        {t('sso_providers.drawer.metadata.hint')}
      </span>
    </div>
  );
}

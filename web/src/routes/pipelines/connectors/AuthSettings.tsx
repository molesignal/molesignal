import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as connectorsApi from '@/api/connectors';
import type { Connector } from '@/api/connectors';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import { FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';

import { keepsStored, storedFor, type ConnectorDraft } from './draft';
import { AUTH_FIELDS, BASE_KEY_FIELDS, keepsSecrets, specOf, type AuthField, type AuthMode } from './kinds';
import { PushSetup } from './PushSetup';
import { newPushToken } from './pushToken';
import { Segmented } from '../editor/Segmented';

/** More ways than this are offered in a list rather than side by side, where their names would be cut. */
const SIDE_BY_SIDE = 3;

/** A titled group of a connector's settings. */
export function SettingsGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="grid gap-[12px]">
      <h3 className="m-0 font-sans text-sm font-strong text-tx-0">{title}</h3>
      {children}
    </section>
  );
}

/** How a connector proves who it is: the way, a word on it, and what it needs filled in. */
export function AuthSettings({
  draft,
  saved,
  onChange,
}: {
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (next: ConnectorDraft) => void;
}) {
  const { t, i18n } = useTranslation('pipelines');
  const spec = specOf(draft.kind);
  const set = (key: string, value: string) => onChange({ ...draft, values: { ...draft.values, [key]: value } });
  // The same way can want a different word of advice from one kind to the next.
  const hintKey = [`connectors.auth.hints.${draft.auth}_${draft.kind}`, `connectors.auth.hints.${draft.auth}`].find(
    (key) => i18n.exists(key, { ns: 'pipelines' }),
  );

  // A client certificate is presented over TLS, so asking for one turns TLS on.
  const pick = (auth: AuthMode) => onChange({ ...draft, auth, tls: auth === 'mtls' ? true : draft.tls });
  const options = spec.auth.map((mode) => ({ value: mode, label: t(`connectors.auth.modes.${mode}`) }));

  const field = (item: AuthField) => (
    <CredentialField
      key={item.key}
      field={item}
      value={draft.values[item.key] ?? ''}
      stored={storedFor(draft, saved, item.key)}
      keeps={saved ? keepsSecrets(saved) : true}
      onChange={(value) => set(item.key, value)}
    />
  );

  return (
    <SettingsGroup title={t('connectors.auth.label')}>
      {spec.auth.length > SIDE_BY_SIDE ? (
        <FormSelect
          value={draft.auth}
          onChange={(mode) => pick(mode as AuthMode)}
          ariaLabel={t('connectors.auth.label')}
          options={options}
        />
      ) : (
        spec.auth.length > 1 && (
          <Segmented<AuthMode> label={t('connectors.auth.label')} value={draft.auth} onChange={pick} options={options} />
        )
      )}
      {hintKey && (
        <p data-connector-auth-hint className="m-0 font-sans text-xs leading-relaxed text-tx-3">
          {t(hintKey)}
        </p>
      )}
      {draft.auth === 'push_token' ? (
        <PushToken draft={draft} saved={saved} onChange={set} />
      ) : (
        <>
          {AUTH_FIELDS[draft.auth].map(field)}
          {draft.auth === 'assume_role' && (
            <>
              <ExternalId />
              <div className="grid gap-[12px]">
                <div>
                  <label className="flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
                    <Checkbox
                      checked={draft.baseKey}
                      onCheckedChange={(checked) => onChange({ ...draft, baseKey: checked === true })}
                    />
                    {t('connectors.auth.base_key')}
                  </label>
                  <p className="m-0 mt-1 pl-[26px] font-sans text-xs leading-relaxed text-tx-3">
                    {t('connectors.auth.base_key_hint')}
                  </p>
                </div>
                {draft.baseKey && <div className="grid gap-[12px] pl-[26px]">{BASE_KEY_FIELDS.map(field)}</div>}
              </div>
            </>
          )}
        </>
      )}
    </SettingsGroup>
  );
}

function CredentialField({
  field,
  value,
  stored,
  keeps,
  onChange,
}: {
  field: AuthField;
  value: string;
  /** The saved connector holds this secret. */
  stored: boolean;
  /** The backend keeps a stored secret that is left empty. */
  keeps: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const secretStored = field.secret && stored;
  const kept = secretStored && keeps;
  const hint = !secretStored ? undefined : keeps ? t('connectors.secret.saved') : t('connectors.secret.retype');
  // A saved secret is not shown, only stood in for.
  const placeholder = kept ? '••••••••••••' : field.example;

  return (
    <FormField label={t(`connectors.fields.${field.key}`)} {...(hint ? { hint } : {})} required={!kept}>
      {field.multiline ? (
        <FormTextarea
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          className="min-h-[96px] font-mono text-xs"
          required={!kept}
        />
      ) : (
        <FormInput
          type={field.secret ? 'password' : 'text'}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          // Not a sign-in form: a password manager has nothing to offer or to save here.
          autoComplete={field.secret ? 'new-password' : 'off'}
          className="font-mono"
          required={!kept}
        />
      )}
    </FormField>
  );
}

/** The external ID roles are assumed with: the organization's, to be required in the role's trust policy. */
function ExternalId() {
  const { t } = useTranslation('pipelines');
  const externalId = useQuery({
    queryKey: ['connector-aws-external-id'],
    queryFn: connectorsApi.awsExternalId,
    staleTime: Number.POSITIVE_INFINITY,
  });

  return (
    <div data-connector-external-id className="grid min-w-0 gap-1.5">
      <span className={uiLabelClass}>{t('connectors.auth.external_id')}</span>
      <CopyField
        value={externalId.data ?? ''}
        copyLabel={t('connectors.auth.copy_external_id')}
        disabled={!externalId.data}
      />
      <span className="font-sans text-xs leading-relaxed text-tx-3">{t('connectors.auth.external_id_hint')}</span>
    </div>
  );
}

/**
 * A push connector's token. It is made here and never typed: a new connector
 * gets one when it is created, a saved one keeps its own until a new one is
 * asked for, which then replaces it on save.
 */
function PushToken({
  draft,
  saved,
  onChange,
}: {
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (key: string, value: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const stored = storedFor(draft, saved, 'pushToken');
  const fresh = (draft.values.pushToken ?? '') !== '';

  if (!saved) {
    return (
      <>
        <PushSetup kind={draft.kind} />
        <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">{t('connectors.push.will_generate')}</p>
      </>
    );
  }

  return (
    <>
      <PushSetup kind={draft.kind} />
      <div className="grid gap-1.5">
        <span className={uiLabelClass}>{t('connectors.push.token')}</span>
        <p data-connector-token-state className="m-0 font-sans text-sm leading-relaxed text-tx-1">
          {fresh
            ? t('connectors.push.regenerated')
            : stored
              ? keepsStored(draft, saved, 'pushToken')
                ? t('connectors.push.saved_token')
                : t('connectors.secret.retype')
              : t('connectors.push.no_token')}
        </p>
        <div>
          {fresh ? (
            // With nothing kept there is no current token to go back to.
            keepsStored(draft, saved, 'pushToken') && (
              <ChromeButton size="sm" variant="ghost" onClick={() => onChange('pushToken', '')}>
                {t('connectors.push.keep')}
              </ChromeButton>
            )
          ) : (
            <ChromeButton size="sm" onClick={() => onChange('pushToken', newPushToken())}>
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('connectors.push.regenerate')}
            </ChromeButton>
          )}
        </div>
      </div>
    </>
  );
}

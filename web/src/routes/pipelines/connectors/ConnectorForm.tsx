import { RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { Connector } from '@/api/connectors';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { Switch } from '@/shell/ui/switch';

import {
  AUTH_FIELDS,
  authModeOf,
  hasStoredSecret,
  keepsSecrets,
  specOf,
  type AuthMode,
  type ConnectorDraft,
  type FieldSpec,
} from './kinds';
import { PushSetup } from './PushSetup';
import { newPushToken } from './pushToken';
import { usePipelineResources } from '../editor/resources';
import { Segmented } from '../editor/Segmented';
import { StreamCombobox } from '../editor/StreamCombobox';
import { pipelineStreamOptions } from '../PipelineGraph/streamOptions';

/**
 * A connector's settings: its name, what it reads or where it delivers, and
 * how it authenticates. `saved` is the connector being changed; its secrets
 * cannot be read back, so each is either left as it is or typed anew.
 */
export function ConnectorForm({
  draft,
  saved,
  onChange,
}: {
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (next: ConnectorDraft) => void;
}) {
  const { t } = useTranslation('pipelines');
  const spec = specOf(draft.kind);
  const set = (key: string, value: string) => onChange({ ...draft, values: { ...draft.values, [key]: value } });
  const fields = spec.fields.filter((field) => !field.compatible);

  return (
    <div className="grid gap-[20px]">
      <FormField label={t('connectors.fields.name')} required>
        <FormInput
          value={draft.name}
          onChange={(event) => onChange({ ...draft, name: event.target.value })}
          placeholder={t('connectors.fields.name_placeholder')}
          autoComplete="off"
          required
        />
      </FormField>

      <Group title={t(spec.direction === 'source' ? 'connectors.drawer.reads' : 'connectors.drawer.delivers_to')}>
        {fields.map((field) => (
          <SettingField key={field.key} field={field} draft={draft} onChange={set} />
        ))}
        {draft.kind === 's3' && <CompatibleStorage draft={draft} onChange={onChange} />}
      </Group>

      <Group title={t('connectors.auth.label')}>
        {spec.auth.length > 1 && (
          <Segmented<AuthMode>
            label={t('connectors.auth.label')}
            value={draft.auth}
            onChange={(auth) => onChange({ ...draft, auth })}
            options={spec.auth.map((mode) => ({ value: mode, label: t(`connectors.auth.modes.${mode}`) }))}
          />
        )}
        <p data-connector-auth-hint className="m-0 font-sans text-xs leading-relaxed text-tx-3">
          {t(`connectors.auth.hints.${draft.auth}`)}
        </p>
        {draft.auth === 'push_token' ? (
          <PushToken draft={draft} saved={saved} onChange={set} />
        ) : (
          AUTH_FIELDS[draft.auth].map((field) => (
            <CredentialField
              key={field.key}
              name={field.key}
              secret={field.secret}
              value={draft.values[field.key] ?? ''}
              stored={Boolean(saved) && storedFor(draft, saved, field.key)}
              keeps={saved ? keepsSecrets(saved) : true}
              onChange={(value) => set(field.key, value)}
            />
          ))
        )}
      </Group>

      {saved && (
        <label className="flex cursor-pointer items-center gap-[10px] font-sans text-sm text-tx-1">
          <Switch checked={draft.enabled} onCheckedChange={(enabled) => onChange({ ...draft, enabled })} />
          {t('connectors.fields.enabled')}
        </label>
      )}
    </div>
  );
}

/** Whether the saved connector holds this secret for the way of authenticating now chosen. */
function storedFor(draft: ConnectorDraft, saved: Connector | undefined, key: string): boolean {
  return (
    saved !== undefined &&
    saved.kind === draft.kind &&
    hasStoredSecret(saved, key) &&
    AUTH_FIELDS[authModeOf(saved)].some((field) => field.key === key)
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="grid gap-[12px]">
      <h3 className="m-0 font-sans text-sm font-strong text-tx-0">{title}</h3>
      {children}
    </section>
  );
}

function SettingField({
  field,
  draft,
  onChange,
}: {
  field: FieldSpec;
  draft: ConnectorDraft;
  onChange: (key: string, value: string) => void;
}) {
  const { t, i18n } = useTranslation('pipelines');
  const resources = usePipelineResources();
  const value = draft.values[field.key] ?? '';
  // The same setting can want a different word of advice from one kind to the next.
  const hintKey = [`connectors.hints.${field.key}_${draft.kind}`, `connectors.hints.${field.key}`].find((key) =>
    i18n.exists(key, { ns: 'pipelines' }),
  );

  return (
    <FormField
      label={t(`connectors.fields.${field.key}`)}
      {...(hintKey ? { hint: t(hintKey) } : {})}
      required={field.required ?? false}
    >
      {field.stream ? (
        <StreamCombobox
          value={value}
          // A source connector brings logs in, so it writes to a log stream.
          streams={pipelineStreamOptions(resources.streams, 'logs', value)}
          pending={resources.streamsPending}
          creatable
          onChange={(stream) => onChange(field.key, stream)}
        />
      ) : (
        <FormInput
          value={value}
          onChange={(event) => onChange(field.key, event.target.value)}
          placeholder={field.example}
          autoComplete="off"
          className="font-mono text-xs"
          required={field.required ?? false}
        />
      )}
    </FormField>
  );
}

/** The settings only an S3-compatible store needs, kept out of the way of a plain AWS bucket. */
function CompatibleStorage({
  draft,
  onChange,
}: {
  draft: ConnectorDraft;
  onChange: (next: ConnectorDraft) => void;
}) {
  const { t } = useTranslation('pipelines');
  const endpoint = specOf('s3').fields.find((field) => field.key === 'endpoint');

  return (
    <div className="grid gap-[12px]">
      <div>
        <label className="flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
          <Checkbox
            checked={draft.compatible}
            onCheckedChange={(checked) => onChange({ ...draft, compatible: checked === true })}
          />
          {t('connectors.compatible.toggle')}
        </label>
        <p className="m-0 mt-1 pl-[26px] font-sans text-xs leading-relaxed text-tx-3">
          {t('connectors.compatible.hint')}
        </p>
      </div>
      {draft.compatible && endpoint && (
        <div className="grid gap-[12px] pl-[26px]">
          <SettingField
            field={endpoint}
            draft={draft}
            onChange={(key, value) => onChange({ ...draft, values: { ...draft.values, [key]: value } })}
          />
          <div>
            <label className="flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
              <Checkbox
                checked={draft.virtualHostedStyle}
                onCheckedChange={(checked) => onChange({ ...draft, virtualHostedStyle: checked === true })}
              />
              {t('connectors.compatible.virtual_hosted')}
            </label>
            <p className="m-0 mt-1 pl-[26px] font-sans text-xs leading-relaxed text-tx-3">
              {t('connectors.compatible.virtual_hosted_hint')}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

function CredentialField({
  name,
  secret,
  value,
  stored,
  keeps,
  onChange,
}: {
  name: string;
  secret: boolean;
  value: string;
  /** The saved connector holds this secret. */
  stored: boolean;
  /** The backend keeps a stored secret that is left empty. */
  keeps: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation('pipelines');
  const kept = stored && keeps;
  const hint = !stored ? undefined : keeps ? t('connectors.secret.saved') : t('connectors.secret.retype');

  return (
    <FormField label={t(`connectors.fields.${name}`)} {...(hint ? { hint } : {})} required={!kept}>
      <FormInput
        type={secret ? 'password' : 'text'}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        // A saved secret is not shown, only stood in for.
        placeholder={kept ? '••••••••••••' : undefined}
        // Not a sign-in form: a password manager has nothing to offer or to save here.
        autoComplete={secret ? 'new-password' : 'off'}
        className="font-mono text-xs"
        required={!kept}
      />
    </FormField>
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
  const stored = storedFor(draft, saved, 'push_token');
  const fresh = (draft.values.push_token ?? '') !== '';

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
              ? keepsSecrets(saved)
                ? t('connectors.push.saved_token')
                : t('connectors.secret.retype')
              : t('connectors.push.no_token')}
        </p>
        <div>
          {fresh ? (
            // With nothing kept there is no current token to go back to.
            stored && keepsSecrets(saved) && (
              <ChromeButton size="sm" variant="ghost" onClick={() => onChange('push_token', '')}>
                {t('connectors.push.keep')}
              </ChromeButton>
            )
          ) : (
            <ChromeButton size="sm" onClick={() => onChange('push_token', newPushToken())}>
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('connectors.push.regenerate')}
            </ChromeButton>
          )}
        </div>
      </div>
    </>
  );
}

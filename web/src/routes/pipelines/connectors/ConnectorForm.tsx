import { useTranslation } from 'react-i18next';

import type { Connector } from '@/api/connectors';
import { FormField, FormInput, FormRow, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { Switch } from '@/shell/ui/switch';

import { AuthSettings, SettingsGroup } from './AuthSettings';
import { ConnectionTest } from './ConnectionTest';
import { keepsStored, storedFor, type ConnectorDraft } from './draft';
import { isKafkaKind, isS3Kind, specOf, type FieldSpec } from './kinds';
import { ReadStateNote } from './ReadState';
import { usePipelineResources } from '../editor/resources';
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
  const fields = spec.fields.filter((field) => !field.compatible && !field.tls);
  // Settings picked from a few short choices sit side by side.
  const typed = fields.filter((field) => !field.choices);
  const picked = fields.filter((field) => field.choices);

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

      <SettingsGroup title={t(spec.direction === 'source' ? 'connectors.drawer.reads' : 'connectors.drawer.delivers_to')}>
        {typed.map((field) => (
          <SettingField key={field.key} field={field} draft={draft} saved={saved} onChange={set} />
        ))}
        {picked.length > 0 && (
          <FormRow>
            {picked.map((field) => (
              <SettingField key={field.key} field={field} draft={draft} saved={saved} onChange={set} />
            ))}
          </FormRow>
        )}
        {isS3Kind(draft.kind) && <CompatibleStorage draft={draft} saved={saved} onChange={onChange} />}
        {isKafkaKind(draft.kind) && <TlsSettings draft={draft} saved={saved} onChange={onChange} />}
      </SettingsGroup>

      <AuthSettings draft={draft} saved={saved} onChange={onChange} />

      {saved && <ReadStateNote connector={saved} />}

      {spec.transport !== 'push' && <ConnectionTest draft={draft} saved={saved} />}

      {saved && (
        <label className="flex cursor-pointer items-center gap-[10px] font-sans text-sm text-tx-1">
          <Switch checked={draft.enabled} onCheckedChange={(enabled) => onChange({ ...draft, enabled })} />
          {t('connectors.fields.enabled')}
        </label>
      )}
    </div>
  );
}

function SettingField({
  field,
  draft,
  saved,
  onChange,
}: {
  field: FieldSpec;
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (key: string, value: string) => void;
}) {
  const { t, i18n } = useTranslation('pipelines');
  const resources = usePipelineResources();
  const value = draft.values[field.key] ?? '';
  const stored = Boolean(field.secret) && storedFor(draft, saved, field.key);
  const kept = stored && keepsStored(draft, saved, field.key);
  // The same setting can want a different word of advice from one kind to the next.
  const hintKey = [`connectors.hints.${field.key}_${draft.kind}`, `connectors.hints.${field.key}`].find((key) =>
    i18n.exists(key, { ns: 'pipelines' }),
  );
  const savedHint = !stored ? undefined : kept ? t('connectors.secret.saved') : t('connectors.secret.retype');
  const hint = [savedHint, hintKey ? t(hintKey) : undefined].filter(Boolean).join(' ');
  const required = (field.required ?? false) && !kept;

  return (
    <FormField label={t(`connectors.fields.${field.key}`)} {...(hint ? { hint } : {})} required={required}>
      {field.stream ? (
        <StreamCombobox
          value={value}
          // A source connector brings logs in, so it writes to a log stream.
          streams={pipelineStreamOptions(resources.streams, 'logs', value)}
          pending={resources.streamsPending}
          creatable
          onChange={(stream) => onChange(field.key, stream)}
        />
      ) : field.choices ? (
        <FormSelect
          value={value || (field.choices[0] ?? '')}
          onChange={(choice) => onChange(field.key, choice)}
          ariaLabel={t(`connectors.fields.${field.key}`)}
          options={field.choices.map((choice) => ({
            value: choice,
            label: t(`connectors.choices.${field.key}.${choice}`),
          }))}
        />
      ) : field.multiline ? (
        <FormTextarea
          value={value}
          onChange={(event) => onChange(field.key, event.target.value)}
          placeholder={kept ? '••••••••••••' : field.example}
          spellCheck={false}
          autoComplete="off"
          className="min-h-[96px] font-mono text-xs"
          required={required}
        />
      ) : (
        <FormInput
          type={field.secret ? 'password' : 'text'}
          value={value}
          onChange={(event) => onChange(field.key, event.target.value)}
          // A saved secret is not shown, only stood in for.
          placeholder={kept ? '••••••••••••' : field.example}
          autoComplete={field.secret ? 'new-password' : 'off'}
          className="font-mono"
          required={required}
        />
      )}
    </FormField>
  );
}

/** A switch with its word of advice, and what it opens below it. */
function Disclosure({
  checked,
  label,
  hint,
  onCheckedChange,
  children,
}: {
  checked: boolean;
  label: string;
  hint: string;
  onCheckedChange: (checked: boolean) => void;
  children?: React.ReactNode;
}) {
  return (
    <div className="grid gap-[12px]">
      <div>
        <label className="flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
          <Checkbox checked={checked} onCheckedChange={(next) => onCheckedChange(next === true)} />
          {label}
        </label>
        <p className="m-0 mt-1 pl-[26px] font-sans text-xs leading-relaxed text-tx-3">{hint}</p>
      </div>
      {checked && children && <div className="grid gap-[12px] pl-[26px]">{children}</div>}
    </div>
  );
}

/** The settings only an S3-compatible store needs, kept out of the way of a plain AWS bucket. */
function CompatibleStorage({
  draft,
  saved,
  onChange,
}: {
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (next: ConnectorDraft) => void;
}) {
  const { t } = useTranslation('pipelines');
  const endpoint = specOf(draft.kind).fields.find((field) => field.key === 'endpoint');

  return (
    <Disclosure
      checked={draft.compatible}
      label={t('connectors.compatible.toggle')}
      hint={t('connectors.compatible.hint')}
      onCheckedChange={(compatible) => onChange({ ...draft, compatible })}
    >
      {endpoint && (
        <SettingField
          field={endpoint}
          draft={draft}
          saved={saved}
          onChange={(key, value) => onChange({ ...draft, values: { ...draft.values, [key]: value } })}
        />
      )}
      <Disclosure
        checked={draft.virtualHostedStyle}
        label={t('connectors.compatible.virtual_hosted')}
        hint={t('connectors.compatible.virtual_hosted_hint')}
        onCheckedChange={(virtualHostedStyle) => onChange({ ...draft, virtualHostedStyle })}
      />
    </Disclosure>
  );
}

/** Whether the connection to the brokers is encrypted, and the CA to trust when it is a private one. */
function TlsSettings({
  draft,
  saved,
  onChange,
}: {
  draft: ConnectorDraft;
  saved?: Connector | undefined;
  onChange: (next: ConnectorDraft) => void;
}) {
  const { t } = useTranslation('pipelines');
  const ca = specOf(draft.kind).fields.find((field) => field.key === 'tlsCaCert');

  return (
    <Disclosure
      checked={draft.tls}
      label={t('connectors.tls.toggle')}
      hint={t('connectors.tls.hint')}
      onCheckedChange={(tls) => onChange({ ...draft, tls })}
    >
      {ca && (
        <SettingField
          field={ca}
          draft={draft}
          saved={saved}
          onChange={(key, value) => onChange({ ...draft, values: { ...draft.values, [key]: value } })}
        />
      )}
    </Disclosure>
  );
}

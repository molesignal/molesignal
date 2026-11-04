import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import * as templatesApi from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { CopyableValue, ReadOnlyValue } from '@/routes/settings/_atoms';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { JsonEditor } from '@/shell/JsonEditor';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { channelOf, orderConnectorTypes } from './channels';
import {
  HTTP_SMS_PLACEHOLDERS,
  PHONE_CONNECTOR_TYPES,
  REGION_SETS,
  buildConfig,
  credentialsComplete,
  defaultTargetType,
  fieldSpecs,
  fieldsFrom,
  regionQuery,
  rowsOf,
  unparsedJson,
  visibleFields,
  withValue,
  type FieldSpec,
} from './fields';

/** `value` once it has stayed the same for `ms`: a list asked for with credentials waits for the typing to stop. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = React.useState(value);

  React.useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms]);

  return settled;
}

/** The regions a regional connector can use, as its provider listed them, and how the asking went. */
interface RegionChoices {
  regions: notifyApi.NotifyConnectorRegion[];
  /** Why there is no list yet: still asking, the settings are not enough to ask, or the provider refused. */
  note: string;
  failed: boolean;
}

export function ConnectorEditor({
  open,
  connector,
  connectorTypes,
  onClose,
}: {
  open: boolean;
  connector: notifyApi.NotifyConnector | null;
  connectorTypes: notifyApi.NotifyConnectorType[];
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('notify');
  const qc = useQueryClient();
  const language = i18n.resolvedLanguage ?? i18n.language;
  // By channel, in the order of the interface language; a new connector starts at the first.
  const ordered = React.useMemo(
    () => orderConnectorTypes(connectorTypes.map((type) => type.connectorType), language),
    [connectorTypes, language],
  );
  const initialType = connector?.connectorType ?? ordered[0]?.connectorType ?? 'email_smtp';
  const [name, setName] = React.useState('');
  const [connectorType, setConnectorType] = React.useState(initialType);
  const [enabled, setEnabled] = React.useState(true);
  const [fields, setFields] = React.useState<Record<string, string>>({});
  const [targetType, setTargetType] = React.useState<notifyApi.NotifyTargetType>('direct_user');
  const [target, setTarget] = React.useState('');
  const [testTemplateId, setTestTemplateId] = React.useState('');
  // A type's settings, with the provider's addresses the backend lists for it.
  const specsOf = React.useCallback(
    (type: string) => fieldSpecs(type, connectorTypes.find((entry) => entry.connectorType === type)?.addresses),
    [connectorTypes],
  );

  React.useEffect(() => {
    if (!open) return;
    const nextType = connector?.connectorType ?? ordered[0]?.connectorType ?? 'email_smtp';
    setName(connector?.name ?? '');
    setConnectorType(nextType);
    setEnabled(connector?.enabled ?? true);
    setFields(fieldsFrom(specsOf(nextType), connector));
    setTargetType(defaultTargetType(nextType));
    setTarget('');
    setTestTemplateId('');
  }, [connector, ordered, open, specsOf]);

  const all = specsOf(connectorType);

  // A type that sends only approved templates is tested with a template of its channel that carries one;
  // an IM app may be tested with one of the instant messaging templates, to see its card.
  const provider = notifyApi.templateProvider(connectorType);
  const cards = notifyApi.sendsCards(connectorType);
  const channel = channelOf(connectorType);
  const templates = useQuery({
    queryKey: ['notify', 'templates'],
    queryFn: templatesApi.list,
    enabled: open && (provider !== null || cards),
  });
  const testTemplates = (templates.data ?? []).filter(
    (template) =>
      template.channel === channel &&
      (provider === null || Boolean(template.providerTemplates[provider])),
  );
  // An approved-template type cannot go without one; an IM app goes without one unless it is chosen.
  const testTemplate =
    testTemplates.find((template) => template.id === testTemplateId) ??
    (provider !== null ? testTemplates[0] : undefined);

  // The regions the provider lists, asked once the settings to ask with have settled.
  const asked = regionQuery(connectorType, all, fields);
  const askedWith = useSettled(asked ? JSON.stringify(asked) : '', 500);
  const regions = useQuery({
    queryKey: ['notify', 'connector-regions', connectorType, connector?.id ?? '', askedWith, language],
    queryFn: () =>
      notifyApi.listConnectorRegions({
        connectorType: connectorType,
        config: { ...(JSON.parse(askedWith) as Record<string, string>), region: fields.region ?? '' },
        ...(connector ? { connectorId: connector.id } : {}),
        language,
      }),
    enabled: open && askedWith !== '',
    staleTime: 10 * 60 * 1000,
    retry: false,
  });

  // A chosen region decides the address: keep it in step once the list is in.
  const regionAddress = regions.data?.find((region) => region.id === fields.region)?.apiBaseUrl;
  React.useEffect(() => {
    if (regionAddress === undefined) return;
    setFields((current) =>
      current[REGION_SETS] === regionAddress ? current : { ...current, [REGION_SETS]: regionAddress },
    );
  }, [regionAddress]);

  const regionChoices: RegionChoices = {
    regions: regions.data ?? [],
    note:
      asked === null
        ? t('connectors.drawer.regions_need_credentials')
        : regions.isError
          ? t('connectors.drawer.regions_failed', { error: toApiError(regions.error).message })
          : regions.isFetching || askedWith === ''
            ? t('connectors.drawer.regions_loading')
            : '',
    failed: regions.isError,
  };

  const save = useMutation({
    mutationFn: () => {
      const config = buildConfig(all, fields);
      return connector
        ? notifyApi.updateConnector(connector.id, { name, config, enabled })
        : notifyApi.createConnector({ name, connectorType: connectorType, config, enabled });
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'connectors'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const test = useMutation({
    mutationFn: () =>
      notifyApi.testConnector(connector?.id ?? '', targetType, target, testTemplate?.id),
    onSuccess: (result) => {
      if (result.sent) toast.success(t('common.test_sent'));
      else toast.error(result.error ?? t('common.test_failed'));
      void qc.invalidateQueries({ queryKey: ['notify', 'connectors'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const specs = visibleFields(all, fields);
  // What a type needs from its provider and what its targets look like; not every type has one.
  // A placeholder the hint names is passed in as itself, so it is shown rather than filled.
  const hint = t(`connectors.drawer.hints.${connectorType}`, {
    defaultValue: '',
    ...Object.fromEntries(HTTP_SMS_PLACEHOLDERS.map((name) => [name, `{{${name}}}`])),
  });
  const valid =
    name.trim() !== '' &&
    specs.every((field) => field.optional || (fields[field.key] ?? '').trim() !== '') &&
    credentialsComplete(connectorType, fields);

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={connector ? t('connectors.drawer.edit_title', { name: connector.name }) : t('connectors.drawer.new_title')}
      subtitle={t('connectors.drawer.subtitle')}
      footer={
        <>
          <ChromeButton className="h-11 md:h-9" onClick={onClose}>
            {t('common.cancel')}
          </ChromeButton>
          <ChromeButton
            className="h-11 md:h-9"
            disabled={
              !connector ||
              target.trim() === '' ||
              (provider !== null && !testTemplate) ||
              test.isPending
            }
            disabledReason={!connector ? t('connectors.drawer.save_before_test') : undefined}
            onClick={() => test.mutate()}
          >
            {t('connectors.drawer.test')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            className="h-11 md:h-9"
            disabled={!valid || save.isPending}
            onClick={() => {
              const broken = unparsedJson(all, fields);
              if (broken) {
                toast.error(t('common.json_error', { field: fieldLabel(t, connectorType, broken) }));
                return;
              }
              save.mutate();
            }}
          >
            {save.isPending ? t('common.saving') : t('connectors.drawer.save')}
          </ChromeButton>
        </>
      }
    >
      <FormSection title={t('connectors.drawer.identity')}>
        <FormField label={t('connectors.drawer.name')} required>
          <FormInput className="h-11 md:h-9" value={name} onChange={(event) => setName(event.target.value)} />
        </FormField>
        {connector ? (
          // A saved connector keeps its type: its settings and the endpoints bound to it are that type's.
          <ReadOnlyField name="type" label={t('connectors.drawer.type')}>
            <ReadOnlyValue value={t(`connector_types.${connectorType}`, { defaultValue: connectorType })} />
          </ReadOnlyField>
        ) : (
          <FormField label={t('connectors.drawer.type')} required>
            <FormSelect
              value={connectorType}
              onChange={(value) => {
                setConnectorType(value);
                setFields(fieldsFrom(specsOf(value)));
                setTargetType(defaultTargetType(value));
              }}
              options={ordered.map(({ channel, connectorType: value }) => ({
                value,
                label: t(`connector_types.${value}`, { defaultValue: value }),
                group: t(`connector_channels.${channel}`),
              }))}
              className="h-11 md:h-9"
            />
          </FormField>
        )}
        <SwitchRow label={t('connectors.drawer.enabled')} checked={enabled} onCheckedChange={setEnabled} />
      </FormSection>

      <FormSection
        title={t('connectors.drawer.configuration')}
        {...(hint ? { description: hint } : {})}
      >
        {rowsOf(specs).map((row) =>
          row.length === 2 ? (
            <FormRow key={row[0]!.key} className="grid-cols-1 md:grid-cols-2">
              {renderField(row[0]!, connectorType, all, fields, setFields, regionChoices, t)}
              {renderField(row[1]!, connectorType, all, fields, setFields, regionChoices, t)}
            </FormRow>
          ) : (
            renderField(row[0]!, connectorType, all, fields, setFields, regionChoices, t)
          ),
        )}
        {(connectorType === 'wecom_app' ||
          (fields.replies === 'true' && specs.some((field) => field.key === 'replies'))) && (
          <ReplyCallbacks connectorId={connector?.id} connectorType={connectorType} />
        )}
      </FormSection>

      <FormSection title={t('connectors.drawer.test_target')}>
        <FormRow className="grid-cols-1 md:grid-cols-[180px_minmax(0,1fr)]">
          <FormField label={t('connectors.drawer.target_type')}>
            <FormSelect
              value={targetType}
              onChange={(value) => setTargetType(value as notifyApi.NotifyTargetType)}
              options={['direct_user', 'fixed_address', 'fixed_group', 'webhook']}
              className="h-11 md:h-9"
            />
          </FormField>
          <FormField
            label={t('connectors.drawer.target')}
            {...(PHONE_CONNECTOR_TYPES.has(connectorType) ? { hint: t('endpoints.phone_hint') } : {})}
          >
            <FormInput className="h-11 md:h-9" value={target} onChange={(event) => setTarget(event.target.value)} />
          </FormField>
        </FormRow>
        {provider !== null && channel !== null && (
          <FormField
            label={t('connectors.drawer.test_template')}
            hint={
              testTemplates.length > 0 || templates.isPending
                ? t('connectors.drawer.test_template_hint')
                : t('connectors.drawer.test_template_none', {
                    channel: t(`connector_channels.${channel}`),
                  })
            }
          >
            <FormSelect
              value={testTemplate?.id ?? ''}
              onChange={setTestTemplateId}
              options={testTemplates.map((template) => ({
                value: template.id,
                label: template.name,
              }))}
              disabled={testTemplates.length === 0}
              className="h-11 md:h-9"
            />
          </FormField>
        )}
        {cards && testTemplates.length > 0 && (
          <FormField
            label={t('connectors.drawer.test_template')}
            hint={t('connectors.drawer.test_card_hint')}
          >
            <FormSelect
              value={testTemplate?.id ?? ''}
              onChange={setTestTemplateId}
              options={[
                { value: '', label: t('connectors.drawer.test_without_template') },
                ...testTemplates.map((template) => ({
                  value: template.id,
                  label: template.name,
                })),
              ]}
              className="h-11 md:h-9"
            />
          </FormField>
        )}
      </FormSection>
    </FormDrawer>
  );
}

/** A setting that is on or off, its name beside the switch. */
function SwitchRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex min-h-11 items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
      <span>{label}</span>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </label>
  );
}

/** Where the provider sends replies, keypresses and a WeCom app's button presses, to be set in its
 * console. A connector not saved yet has no address, and each Twilio call carries its own. */
function ReplyCallbacks({ connectorId, connectorType }: { connectorId: string | undefined; connectorType: string }) {
  const { t } = useTranslation('notify');
  const callbacks = useQuery({
    queryKey: ['notify', 'connector-callbacks', connectorId],
    queryFn: () => notifyApi.connectorCallbacks(connectorId ?? ''),
    enabled: connectorId !== undefined,
    staleTime: Number.POSITIVE_INFINITY,
  });
  const url =
    callbacks.data?.repliesUrl ?? callbacks.data?.keypressUrl ?? callbacks.data?.messageUrl ?? null;
  const note =
    callbacks.data?.externalUrlSet === false
      ? t('connectors.drawer.callbacks.no_external_url')
      : connectorType === 'twilio_voice'
        ? t('connectors.drawer.callbacks.per_call')
        : connectorId === undefined
          ? t('connectors.drawer.callbacks.after_save')
          : null;

  return (
    <ReadOnlyField
      name="callbacks"
      label={t('connectors.drawer.callbacks.label')}
      hint={note === null ? t(`connectors.drawer.callbacks.hints.${connectorType}`, { defaultValue: '' }) : ''}
    >
      {note !== null ? (
        <ReadOnlyValue value={note} muted />
      ) : url ? (
        <CopyableValue value={url} copyLabel={t('connectors.drawer.copy')} copiedLabel={t('connectors.drawer.copied')} />
      ) : (
        <ReadOnlyValue value="—" muted />
      )}
    </ReadOnlyField>
  );
}

/** A setting shown but not edited, laid out as a field is. Not a `<label>`: a copy button inside one
 * would be pressed by a click on the field's name. */
function ReadOnlyField({
  name,
  label,
  hint,
  children,
}: {
  name: string;
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div data-connector-field={name} className="flex min-w-0 flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      {children}
      {hint && <span className="font-sans text-xs leading-relaxed text-tx-3">{hint}</span>}
    </div>
  );
}

/** The external ID this organization's AWS connectors assume roles with: the role's trust policy requires it. */
function AwsExternalId() {
  const { t } = useTranslation('notify');
  const externalId = useQuery({
    queryKey: ['notify', 'aws-external-id'],
    queryFn: notifyApi.awsExternalId,
    staleTime: Number.POSITIVE_INFINITY,
  });

  return (
    <ReadOnlyField
      name="external_id"
      label={t('connectors.drawer.field.external_id')}
      hint={t('connectors.drawer.external_id_hint')}
    >
      <CopyableValue
        value={externalId.data ?? '—'}
        copyLabel={t('connectors.drawer.copy_external_id')}
        copiedLabel={t('connectors.drawer.copied')}
      />
    </ReadOnlyField>
  );
}

/** A field's name: some types name a shared setting their own way, as a gateway names its URL. */
function fieldLabel(t: TFunction<'notify'>, connectorType: string, key: string): string {
  return t(`connectors.drawer.type_fields.${connectorType}.${key}`, {
    defaultValue: t(`connectors.drawer.field.${key}`),
  });
}

/** A region as its provider names it, with the ID beside a name that is not the ID itself. */
function regionLabel(region: notifyApi.NotifyConnectorRegion): string {
  return region.name === region.id ? region.id : `${region.name} · ${region.id}`;
}

function renderField(
  field: FieldSpec,
  connectorType: string,
  all: FieldSpec[],
  fields: Record<string, string>,
  setFields: React.Dispatch<React.SetStateAction<Record<string, string>>>,
  regions: RegionChoices,
  t: TFunction<'notify'>,
) {
  if (field.type === 'external_id') return <AwsExternalId key={field.key} />;

  const value = fields[field.key] ?? '';
  const choose = (next: string) => setFields((current) => withValue(all, current, field.key, next));

  if (field.type === 'region') {
    // The regions the provider listed; a saved region stays in the list while it is being asked for.
    const listed = regions.regions.map((region) => ({ value: region.id, label: regionLabel(region) }));
    const options =
      value === '' || listed.some((option) => option.value === value) ? listed : [{ value, label: value }, ...listed];

    return (
      <FormField
        key={field.key}
        label={fieldLabel(t, connectorType, field.key)}
        {...(regions.note ? { hint: regions.note } : {})}
      >
        <FormSelect
          value={value}
          onChange={(next) => {
            const address = regions.regions.find((region) => region.id === next)?.apiBaseUrl;
            setFields((current) => ({
              ...current,
              [field.key]: next,
              ...(address === undefined ? {} : { [REGION_SETS]: address }),
            }));
          }}
          options={options}
          ariaLabel={fieldLabel(t, connectorType, field.key)}
          className="h-11 md:h-9"
        />
      </FormField>
    );
  }

  if (field.type === 'switch') {
    // What turning it on does, for this type.
    const said = t(`connectors.drawer.switch_hints.${field.key}.${connectorType}`, { defaultValue: '' });

    return (
      <div key={field.key} data-connector-field={field.key} className="flex min-w-0 flex-col gap-1.5">
        <SwitchRow
          label={fieldLabel(t, connectorType, field.key)}
          checked={value === 'true'}
          onCheckedChange={(on) => choose(on ? 'true' : 'false')}
        />
        {said && <span className="font-sans text-xs leading-relaxed text-tx-3">{said}</span>}
      </div>
    );
  }

  // What the chosen option means, for a choice that needs saying (how an AWS connector signs).
  const hint =
    field.type === 'select'
      ? t(`connectors.drawer.option_hints.${field.key}.${value}`, { defaultValue: '' })
      : '';

  if (field.type === 'fixed') {
    // The provider's address: shown so it can be copied (to allow it through a firewall), not edited.
    return (
      <ReadOnlyField key={field.key} name={field.key} label={fieldLabel(t, connectorType, field.key)}>
        {value ? (
          <CopyableValue
            value={value}
            copyLabel={t('connectors.drawer.copy')}
            copiedLabel={t('connectors.drawer.copied')}
          />
        ) : (
          <ReadOnlyValue value={t('connectors.drawer.address_by_region')} muted />
        )}
      </ReadOnlyField>
    );
  }

  return (
    <FormField key={field.key} label={fieldLabel(t, connectorType, field.key)} {...(hint ? { hint } : {})}>
      {field.type === 'select' ? (
        <FormSelect
          value={value}
          onChange={choose}
          options={(field.options ?? []).map((option) => ({
            value: option,
            label: t(`connectors.drawer.options.${field.key}.${option}`, { defaultValue: option }),
          }))}
          className="h-11 md:h-9"
        />
      ) : field.type === 'textarea' ? (
        // A request written with placeholders, saved as it is written.
        <FormTextarea
          className="min-h-24 font-mono text-base md:text-sm"
          value={value}
          spellCheck={false}
          onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
        />
      ) : field.type === 'json' ? (
        <JsonEditor
          value={value}
          onChange={(next) => setFields((current) => ({ ...current, [field.key]: next }))}
          ariaLabel={fieldLabel(t, connectorType, field.key)}
          minHeight={112}
          // A saved secret comes back as `***`, which is kept as it is: it is no mistake.
          showProblems={value.trim() !== '***'}
        />
      ) : (
        <FormInput
          className="h-11 text-base md:h-9 md:text-sm"
          type={field.type === 'password' ? 'password' : 'text'}
          inputMode={field.type === 'number' ? 'numeric' : undefined}
          value={value}
          onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
        />
      )}
    </FormField>
  );
}

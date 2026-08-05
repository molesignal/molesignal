import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

interface FieldSpec {
  key: string;
  type?: 'text' | 'number' | 'password' | 'json' | 'select';
  options?: string[];
  defaultValue?: string;
}

const FIELDS: Record<string, FieldSpec[]> = {
  email_smtp: [
    { key: 'host' },
    { key: 'port', type: 'number', defaultValue: '587' },
    { key: 'username' },
    { key: 'password', type: 'password' },
    { key: 'from' },
    { key: 'tls', type: 'select', options: ['none', 'starttls', 'tls'], defaultValue: 'starttls' },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
  slack_app: [
    { key: 'bot_token', type: 'password' },
    { key: 'api_base_url', defaultValue: 'https://slack.com/api' },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
  slack_webhook: [
    { key: 'webhook_url', type: 'password' },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
  lark_app: [
    { key: 'app_id' },
    { key: 'app_secret', type: 'password' },
    { key: 'api_base_url', defaultValue: 'https://open.feishu.cn/open-apis' },
    {
      key: 'receive_id_type',
      type: 'select',
      options: ['open_id', 'user_id', 'union_id', 'email'],
      defaultValue: 'open_id',
    },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
  lark_webhook: [
    { key: 'webhook_url', type: 'password' },
    { key: 'secret', type: 'password' },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
  webhook: [
    { key: 'url', type: 'password' },
    { key: 'method', type: 'select', options: ['post', 'put', 'patch'], defaultValue: 'post' },
    { key: 'headers', type: 'json', defaultValue: '{}' },
    { key: 'timeout_secs', type: 'number', defaultValue: '10' },
  ],
};

function fieldsFrom(
  connectorType: string,
  connector?: notifyApi.NotifyConnector | null,
): Record<string, string> {
  return Object.fromEntries(
    (FIELDS[connectorType] ?? []).map((field) => {
      const stored = connector?.config[field.key];
      const value =
        field.type === 'json' && stored && typeof stored === 'object'
          ? JSON.stringify(stored, null, 2)
          : stored === undefined
            ? field.defaultValue ?? ''
            : String(stored);
      return [field.key, value];
    }),
  );
}

function buildConfig(
  connectorType: string,
  values: Record<string, string>,
): Record<string, unknown> {
  return Object.fromEntries(
    (FIELDS[connectorType] ?? []).map((field) => {
      const value = values[field.key] ?? '';
      if (field.type === 'number') return [field.key, Number(value)];
      if (field.type === 'json') {
        return [
          field.key,
          value.trim() === '***' ? '***' : (JSON.parse(value) as unknown),
        ];
      }
      return [field.key, value];
    }),
  );
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
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const initialType = connector?.connector_type ?? connectorTypes[0]?.connector_type ?? 'email_smtp';
  const [name, setName] = React.useState('');
  const [connectorType, setConnectorType] = React.useState(initialType);
  const [enabled, setEnabled] = React.useState(true);
  const [fields, setFields] = React.useState<Record<string, string>>({});
  const [targetType, setTargetType] = React.useState<notifyApi.NotifyTargetType>('direct_user');
  const [target, setTarget] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    const nextType = connector?.connector_type ?? connectorTypes[0]?.connector_type ?? 'email_smtp';
    setName(connector?.name ?? '');
    setConnectorType(nextType);
    setEnabled(connector?.enabled ?? true);
    setFields(fieldsFrom(nextType, connector));
    setTargetType(
      nextType === 'webhook' || nextType.endsWith('_webhook')
        ? 'fixed_group'
        : nextType === 'email_smtp'
          ? 'fixed_address'
          : 'direct_user',
    );
    setTarget('');
  }, [connector, connectorTypes, open]);

  const save = useMutation({
    mutationFn: () => {
      const config = buildConfig(connectorType, fields);
      return connector
        ? notifyApi.updateConnector(connector.id, { name, config, enabled })
        : notifyApi.createConnector({ name, connector_type: connectorType, config, enabled });
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'connectors'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const test = useMutation({
    mutationFn: () => notifyApi.testConnector(connector?.id ?? '', targetType, target),
    onSuccess: (result) => {
      if (result.sent) toast.success(t('common.test_sent'));
      else toast.error(result.error ?? t('common.test_failed'));
      void qc.invalidateQueries({ queryKey: ['notify', 'connectors'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const specs = FIELDS[connectorType] ?? [];
  const valid = name.trim() !== '' && specs.every((field) => {
    if (field.key === 'username' || field.key === 'secret' || field.key === 'headers') return true;
    return (fields[field.key] ?? '').trim() !== '';
  });

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
            disabled={!connector || target.trim() === '' || test.isPending}
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
              try {
                buildConfig(connectorType, fields);
                save.mutate();
              } catch {
                toast.error(t('common.json_error', { field: t('connectors.drawer.field.headers') }));
              }
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
        <FormField label={t('connectors.drawer.type')} required>
          <FormSelect
            value={connectorType}
            disabled={Boolean(connector)}
            onChange={(value) => {
              setConnectorType(value);
              setFields(fieldsFrom(value));
            }}
            options={connectorTypes.map((value) => ({
              value: value.connector_type,
              label: t(`connector_types.${value.connector_type}`, { defaultValue: value.connector_type }),
            }))}
            className="h-11 md:h-9"
          />
        </FormField>
        <label className="flex min-h-11 items-center justify-between gap-4 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1">
          <span>{t('connectors.drawer.enabled')}</span>
          <Switch checked={enabled} onCheckedChange={setEnabled} />
        </label>
      </FormSection>

      <FormSection title={t('connectors.drawer.configuration')}>
        {specs.map((field, index) => {
          const input =
            field.type === 'select' ? (
              <FormSelect
                value={fields[field.key] ?? ''}
                onChange={(value) => setFields((current) => ({ ...current, [field.key]: value }))}
                options={field.options ?? []}
                className="h-11 md:h-9"
              />
            ) : field.type === 'json' ? (
              <FormTextarea
                className="min-h-28 font-mono text-base md:text-sm"
                value={fields[field.key] ?? ''}
                onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
              />
            ) : (
              <FormInput
                className="h-11 text-base md:h-9 md:text-sm"
                type={field.type === 'password' ? 'password' : 'text'}
                inputMode={field.type === 'number' ? 'numeric' : undefined}
                value={fields[field.key] ?? ''}
                onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
              />
            );
          return index % 2 === 0 && specs[index + 1] ? (
            <FormRow key={field.key} className="grid-cols-1 md:grid-cols-2">
              <FormField label={t(`connectors.drawer.field.${field.key}`)}>{input}</FormField>
              {renderField(specs[index + 1]!, fields, setFields, t)}
            </FormRow>
          ) : index % 2 === 1 ? null : (
            <FormField key={field.key} label={t(`connectors.drawer.field.${field.key}`)}>{input}</FormField>
          );
        })}
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
          <FormField label={t('connectors.drawer.target')}>
            <FormInput className="h-11 md:h-9" value={target} onChange={(event) => setTarget(event.target.value)} />
          </FormField>
        </FormRow>
      </FormSection>
    </FormDrawer>
  );
}

function renderField(
  field: FieldSpec,
  fields: Record<string, string>,
  setFields: React.Dispatch<React.SetStateAction<Record<string, string>>>,
  t: TFunction<'notify'>,
) {
  return (
    <FormField key={field.key} label={t(`connectors.drawer.field.${field.key}`)}>
      {field.type === 'select' ? (
        <FormSelect
          value={fields[field.key] ?? ''}
          onChange={(value) => setFields((current) => ({ ...current, [field.key]: value }))}
          options={field.options ?? []}
          className="h-11 md:h-9"
        />
      ) : field.type === 'json' ? (
        <FormTextarea
          className="min-h-28 font-mono text-base md:text-sm"
          value={fields[field.key] ?? ''}
          onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
        />
      ) : (
        <FormInput
          className="h-11 text-base md:h-9 md:text-sm"
          type={field.type === 'password' ? 'password' : 'text'}
          inputMode={field.type === 'number' ? 'numeric' : undefined}
          value={fields[field.key] ?? ''}
          onChange={(event) => setFields((current) => ({ ...current, [field.key]: event.target.value }))}
        />
      )}
    </FormField>
  );
}

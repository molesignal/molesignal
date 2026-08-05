import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as connectorsApi from '@/api/connectors';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { SectionBody } from './_atoms';
import { formatMicros } from '../rum/_helpers';

export function PipelineDestinations() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const navigate = useNavigate();
  const manageAccess = useActionAccess({ permission: 'pipelines.edit' });
  const [creating, setCreating] = React.useState(false);
  const [removing, setRemoving] = React.useState<connectorsApi.Connector | null>(null);

  const q = useQuery({ queryKey: ['connectors'], queryFn: () => connectorsApi.list() });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('pipeline_destinations.empty_title'),
    emptyDescription: t('pipeline_destinations.empty_description'),
  });

  const remove = useMutation({
    mutationFn: (id: string) => connectorsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['connectors'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('pipeline_destinations.title')}
        subtitle={t('pipeline_destinations.subtitle') as string}
        actions={
          <>
            <ChromeButton onClick={() => navigate('/pipelines')}>
              {t('pipeline_destinations.back_to_pipelines')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setCreating(true)}
            >
              {t('pipeline_destinations.new_connector')}
            </ChromeButton>
          </>
        }
      />
      <CreateDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('pipeline_destinations.delete_confirm_title')}
        description={t('pipeline_destinations.delete_confirm_description')}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing.id);
          }
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              { key: 'name', header: t('pipeline_destinations.columns.name'), cell: (r) => r.name },
              {
                key: 'kind',
                header: t('pipeline_destinations.columns.kind'),
                cell: (r) => <Pill tone="blue">{r.kind}</Pill>,
                width: 180,
              },
              {
                key: 'enabled',
                header: t('pipeline_destinations.columns.enabled'),
                cell: (r) =>
                  r.enabled ? (
                    <Pill tone="green">{tc('status.on')}</Pill>
                  ) : (
                    <Pill tone="dim">{tc('status.off')}</Pill>
                  ),
                width: 90,
              },
              {
                key: 'last_run',
                header: t('pipeline_destinations.columns.last_run'),
                cell: (r) => formatMicros(r.last_run_at_micros),
                width: 200,
              },
              {
                key: 'actions',
                header: '',
                width: 80,
                cell: (r) => (
                  <ChromeButton
                    variant="ghost"
                    size="sm"
                    disabled={manageAccess.disabled || remove.isPending}
                    disabledReason={
                      !remove.isPending ? manageAccess.reason : undefined
                    }
                    onClick={(e) => {
                      e.stopPropagation();
                      setRemoving(r);
                    }}
                    className="text-tx-3 enabled:hover:text-red-soft"
                  >
                    {tc('actions.delete')}
                  </ChromeButton>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}

/**
 * 每个 connector kind 的配置字段 - 取代裸 JSON 录入，按所选 kind 渲染专属表单字段。
 * 字段 `key` 必须与后端 `config_json` 读取的键一致（参见 connectors/runner.rs 的 build_pull
 * 与 connectors/mod.rs：shared 字段 target_stream；aws_cloudwatch_logs 另读 region / log_group）。
 */
type ConfigFieldType = 'text' | 'secret';

interface ConfigField {
  key: string;
  /** i18n 后缀，挂在 pipeline_destinations.fields.* */
  label: string;
  type: ConfigFieldType;
  required?: boolean;
  placeholder?: string;
  defaultValue?: string;
  /** i18n 后缀，挂在 pipeline_destinations.hints.* */
  hint?: string;
}

interface ConnectorKindSpec {
  kind: string;
  fields: ConfigField[];
}

// 所有 kind 共享：摄取记录写入的目标 stream。
const TARGET_STREAM_FIELD: ConfigField = {
  key: 'target_stream',
  label: 'target_stream',
  type: 'text',
  required: true,
  placeholder: 'default',
  defaultValue: 'default',
  hint: 'target_stream',
};

// 默认 kind 单独具名，给 specFor 的 fallback 与 DEFAULT_KIND 复用（避免下标访问 undefined）。
const CLOUDWATCH_SPEC: ConnectorKindSpec = {
  kind: 'aws_cloudwatch_logs',
  fields: [
    { key: 'region', label: 'region', type: 'text', required: true, placeholder: 'us-east-1', hint: 'region' },
    { key: 'log_group', label: 'log_group', type: 'text', required: true, placeholder: '/aws/lambda/my-fn', hint: 'log_group' },
    TARGET_STREAM_FIELD,
    { key: 'access_key', label: 'access_key', type: 'secret', hint: 'credentials' },
    { key: 'secret_key', label: 'secret_key', type: 'secret', hint: 'credentials' },
  ],
};

const CONNECTOR_KINDS: ConnectorKindSpec[] = [
  CLOUDWATCH_SPEC,
  { kind: 'aws_kinesis_firehose', fields: [TARGET_STREAM_FIELD] },
  { kind: 'cloudflare_logpush', fields: [TARGET_STREAM_FIELD] },
  { kind: 'heroku_drain', fields: [TARGET_STREAM_FIELD] },
  // s3 / kafka 是 sink（pipeline 投递到此），用各自的目标寻址字段，不带 target_stream。
  {
    kind: 's3',
    fields: [
      { key: 'bucket', label: 'bucket', type: 'text', required: true, placeholder: 'my-log-bucket', hint: 'bucket' },
      { key: 'region', label: 'region', type: 'text', required: true, placeholder: 'us-east-1', hint: 'region' },
      { key: 'prefix', label: 'prefix', type: 'text', placeholder: 'logs/', hint: 'prefix' },
      { key: 'endpoint', label: 'endpoint', type: 'text', placeholder: 'https://s3.amazonaws.com', hint: 'endpoint' },
      { key: 'access_key', label: 'access_key', type: 'secret', hint: 'credentials' },
      { key: 'secret_key', label: 'secret_key', type: 'secret', hint: 'credentials' },
    ],
  },
  {
    kind: 'kafka',
    fields: [
      { key: 'brokers', label: 'brokers', type: 'text', required: true, placeholder: 'broker1:9092', hint: 'brokers' },
      { key: 'topic', label: 'topic', type: 'text', required: true, placeholder: 'logs', hint: 'topic' },
      { key: 'sasl_username', label: 'sasl_username', type: 'text', hint: 'sasl' },
      { key: 'sasl_password', label: 'sasl_password', type: 'secret', hint: 'sasl' },
    ],
  },
];

const DEFAULT_KIND = CLOUDWATCH_SPEC.kind;

function specFor(kind: string): ConnectorKindSpec {
  return CONNECTOR_KINDS.find((k) => k.kind === kind) ?? CLOUDWATCH_SPEC;
}

function defaultConfigFor(kind: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of specFor(kind).fields) out[f.key] = f.defaultValue ?? '';
  return out;
}

/** 从表单字段组装 config_json：trim 后非空才写入，空的可选字段不落库。 */
function buildConfigJson(kind: string, config: Record<string, string>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of specFor(kind).fields) {
    const value = (config[f.key] ?? '').trim();
    if (value) out[f.key] = value;
  }
  return out;
}

function CreateDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [kind, setKind] = React.useState(DEFAULT_KIND);
  const [config, setConfig] = React.useState<Record<string, string>>(() => defaultConfigFor(DEFAULT_KIND));

  React.useEffect(() => {
    if (!open) {
      setName('');
      setKind(DEFAULT_KIND);
      setConfig(defaultConfigFor(DEFAULT_KIND));
    }
  }, [open]);

  // 切换 kind：重置配置到该 kind 的默认值，避免残留上一个 kind 的字段。
  const onKindChange = (next: string) => {
    setKind(next);
    setConfig(defaultConfigFor(next));
  };

  const setField = (key: string, value: string) =>
    setConfig((c) => ({ ...c, [key]: value }));

  const create = useMutation({
    mutationFn: () =>
      connectorsApi.create({ name: name.trim(), kind, config_json: buildConfigJson(kind, config) }),
    onSuccess: () => {
      toast.success(t('pipeline_destinations.toast_created'));
      void qc.invalidateQueries({ queryKey: ['connectors'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const fields = specFor(kind).fields;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('pipeline_destinations.drawer_title')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={!name.trim()}
          disabledReason={access.reason}
          onCancel={onClose}
          submitLabel={t('pipeline_destinations.submit_label')}
          formId="connector-form"
        />
      }
    >
      <form
        id="connector-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (!access.allowed || !name.trim()) return;
          create.mutate();
        }}
      >
        {access.disabled && access.reason && (
          <div
            role="status"
            className="mb-4 rounded-md border border-bd-1 bg-bg-2 px-3 py-2 font-sans text-xs text-tx-2"
          >
            {access.reason}
          </div>
        )}
        <fieldset
          disabled={access.disabled || create.isPending}
          aria-disabled={access.disabled || undefined}
          className="contents"
        >
        <FormSection title={t('pipeline_destinations.section_identity')}>
          <FormField label={t('pipeline_destinations.field_name')} required>
            <FormInput value={name} onChange={(e) => setName(e.target.value)} required />
          </FormField>
          <FormField
            label={t('pipeline_destinations.field_kind')}
            required
            hint={t('pipeline_destinations.field_kind_hint')}
          >
            <FormSelect
              value={kind}
              onChange={onKindChange}
              options={CONNECTOR_KINDS.map((k) => ({
                value: k.kind,
                label: t(`pipeline_destinations.kinds.${k.kind}`),
              }))}
              className="bg-bg-1"
            />
          </FormField>
        </FormSection>
        <FormSection title={t('pipeline_destinations.section_config')} className="mb-0">
          {fields.map((f) => (
            <FormField
              key={f.key}
              label={t(`pipeline_destinations.fields.${f.label}`)}
              hint={f.hint ? t(`pipeline_destinations.hints.${f.hint}`) : ''}
              required={f.required ?? false}
            >
              <FormInput
                value={config[f.key] ?? ''}
                onChange={(e) => setField(f.key, e.target.value)}
                placeholder={f.placeholder}
                required={f.required ?? false}
                type={f.type === 'secret' ? 'password' : 'text'}
                autoComplete={f.type === 'secret' ? 'off' : undefined}
                className={f.type === 'secret' ? 'font-mono' : undefined}
              />
            </FormField>
          ))}
        </FormSection>
        </fieldset>
      </form>
    </FormDrawer>
  );
}

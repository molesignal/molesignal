import { useTranslation } from 'react-i18next';

import type { MonitorKind } from '@/api/synthetics';
import {
  FieldArray,
  FormField,
  FormInput,
  FormRow,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';

import { clientId } from '../model';
import type { AssertionDraft, CheckDraft } from './model';

type DraftPatch = <K extends keyof CheckDraft>(key: K, value: CheckDraft[K]) => void;

export function TargetFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  if (draft.kind === 'http') return <HttpFields draft={draft} patch={patch} />;
  if (draft.kind === 'browser') {
    return (
      <FormField
        label={t('editor.browser_steps')}
        hint={t('editor.browser_steps_hint')}
        required
      >
        <FormTextarea
          className="min-h-48 font-code"
          value={draft.browserSteps}
          onChange={(event) => patch('browserSteps', event.target.value)}
          placeholder={t('editor.browser_steps_placeholder')}
        />
      </FormField>
    );
  }
  if (draft.kind === 'dns') return <DnsFields draft={draft} patch={patch} />;
  if (draft.kind === 'icmp') return <IcmpFields draft={draft} patch={patch} />;
  if (draft.kind === 'tls') return <TlsFields draft={draft} patch={patch} />;
  if (draft.kind === 'grpc') return <GrpcFields draft={draft} patch={patch} />;
  if (draft.kind === 'heartbeat') {
    return (
      <div className="rounded-md border border-bd-0 bg-bg-2 px-4 py-3 text-sm text-tx-2">
        {t('editor.heartbeat_hint')}
      </div>
    );
  }
  return <TcpFields draft={draft} patch={patch} />;
}

function HttpFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <FormRow className="grid-cols-[120px_minmax(0,1fr)]">
        <FormField label={t('editor.method')}>
          <FormSelect
            value={draft.method}
            onChange={(value) => patch('method', value)}
            options={['GET', 'POST', 'PUT', 'PATCH', 'DELETE']}
          />
        </FormField>
        <FormField label={t('editor.url')} required>
          <FormInput
            value={draft.url}
            onChange={(event) => patch('url', event.target.value)}
            placeholder={t('editor.url_placeholder')}
          />
        </FormField>
      </FormRow>
      <FormField label={t('editor.headers')}>
        <FieldArray
          items={draft.headers}
          onChange={(headers) => patch('headers', headers)}
          newItem={() => ({ name: '', value: '' })}
          addLabel={t('actions.add_header')}
          renderItem={(header, _index, setHeader) => (
            <FormRow className="grid-cols-1 sm:grid-cols-2">
              <FormInput
                aria-label={t('editor.header_name')}
                value={header.name}
                onChange={(event) => setHeader({ ...header, name: event.target.value })}
                placeholder={t('editor.header_name')}
              />
              <FormInput
                aria-label={t('editor.header_value')}
                value={header.value}
                onChange={(event) => setHeader({ ...header, value: event.target.value })}
                placeholder={t('editor.header_value')}
              />
            </FormRow>
          )}
        />
      </FormField>
      {draft.method !== 'GET' && draft.method !== 'DELETE' && (
        <FormField label={t('editor.body')}>
          <FormTextarea
            className="font-code"
            value={draft.body}
            onChange={(event) => patch('body', event.target.value)}
            placeholder={t('editor.body_placeholder')}
          />
        </FormField>
      )}
      <CheckToggle
        checked={draft.useTls}
        onChange={(value) => patch('useTls', value)}
        label={t('editor.verify_tls')}
      />
    </>
  );
}

function DnsFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <FormRow className="grid-cols-1 sm:grid-cols-[minmax(0,1fr)_120px]">
        <FormField label={t('editor.host')} required>
          <FormInput
            value={draft.host}
            onChange={(event) => patch('host', event.target.value)}
            placeholder="example.com"
          />
        </FormField>
        <FormField label={t('editor.record_type')}>
          <FormSelect
            value={draft.recordType}
            onChange={(value) => patch('recordType', value)}
            options={['A', 'AAAA', 'MX', 'TXT', 'CNAME', 'NS', 'SOA']}
          />
        </FormField>
      </FormRow>
      <FormField label={t('editor.resolver')}>
        <FormInput
          value={draft.resolver}
          onChange={(event) => patch('resolver', event.target.value)}
          placeholder="1.1.1.1"
        />
      </FormField>
      <FormField
        label={t('editor.expected_values')}
        hint={t('editor.expected_values_hint')}
      >
        <FormInput
          value={draft.expectedValues}
          onChange={(event) => patch('expectedValues', event.target.value)}
        />
      </FormField>
      <CheckToggle
        checked={draft.dnssec}
        onChange={(value) => patch('dnssec', value)}
        label={t('editor.dnssec')}
      />
    </>
  );
}

function IcmpFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <FormField label={t('editor.host')} required>
        <FormInput
          value={draft.host}
          onChange={(event) => patch('host', event.target.value)}
          placeholder="api.example.com"
        />
      </FormField>
      <FormRow className="grid-cols-1 sm:grid-cols-3">
        <FormField label={t('editor.ping_count')}>
          <FormInput
            type="number"
            min={1}
            max={10}
            value={draft.pingCount}
            onChange={(event) => patch('pingCount', event.target.value)}
          />
        </FormField>
        <FormField label={t('editor.packet_loss')}>
          <FormInput
            type="number"
            min={0}
            max={100}
            value={draft.packetLoss}
            onChange={(event) => patch('packetLoss', event.target.value)}
          />
        </FormField>
        <FormField label={t('editor.latency_limit')}>
          <FormInput
            type="number"
            min={1}
            value={draft.latencyLimit}
            onChange={(event) => patch('latencyLimit', event.target.value)}
          />
        </FormField>
      </FormRow>
    </>
  );
}

function TlsFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <HostPortFields draft={draft} patch={patch} />
      <FormField label={t('editor.certificate_days')}>
        <FormSelect
          value={draft.certificateDays}
          onChange={(value) => patch('certificateDays', value)}
          options={['30', '15', '7']}
        />
      </FormField>
    </>
  );
}

function GrpcFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <FormField label={t('editor.url')} required>
        <FormInput
          value={draft.host}
          onChange={(event) => patch('host', event.target.value)}
          placeholder="grpc.example.com:443"
        />
      </FormField>
      <FormField label={t('editor.grpc_service')}>
        <FormInput
          value={draft.grpcService}
          onChange={(event) => patch('grpcService', event.target.value)}
        />
      </FormField>
      <CheckToggle
        checked={draft.useTls}
        onChange={(value) => patch('useTls', value)}
        label={t('editor.tls')}
      />
    </>
  );
}

function TcpFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <HostPortFields draft={draft} patch={patch} />
      <CheckToggle
        checked={draft.useTls}
        onChange={(value) => patch('useTls', value)}
        label={t('editor.tls')}
      />
    </>
  );
}

function HostPortFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <FormRow className="grid-cols-1 sm:grid-cols-[minmax(0,1fr)_120px]">
      <FormField label={t('editor.host')} required>
        <FormInput
          value={draft.host}
          onChange={(event) => patch('host', event.target.value)}
        />
      </FormField>
      <FormField label={t('editor.port')}>
        <FormInput
          type="number"
          value={draft.port}
          onChange={(event) => patch('port', event.target.value)}
        />
      </FormField>
    </FormRow>
  );
}

export function AssertionEditor({
  assertions,
  onChange,
}: {
  assertions: AssertionDraft[];
  onChange: (value: AssertionDraft[]) => void;
}) {
  const { t } = useTranslation('synthetics');
  return (
    <FieldArray
      items={assertions}
      onChange={onChange}
      newItem={(): AssertionDraft => ({
        id: clientId(),
        name: '',
        source: 'status',
        operator: 'equals',
        expected: '',
        severity: 'critical',
      })}
      addLabel={t('actions.add_assertion')}
      renderItem={(assertion, _index, setAssertion) => (
        <div className="grid gap-2 rounded-md border border-bd-0 bg-bg-2 p-3 sm:grid-cols-2">
          <FormInput
            aria-label={t('editor.assertion_source')}
            value={assertion.source}
            onChange={(event) => setAssertion({ ...assertion, source: event.target.value })}
            placeholder="status / body / duration_ms / header:name"
          />
          <FormSelect
            ariaLabel={t('editor.assertion_operator')}
            value={assertion.operator}
            onChange={(operator) =>
              setAssertion({ ...assertion, operator: operator as AssertionDraft['operator'] })
            }
            options={[
              'equals',
              'not_equals',
              'contains',
              'not_contains',
              'matches',
              'less_than',
              'greater_than',
              'exists',
            ]}
          />
          <FormInput
            aria-label={t('editor.assertion_expected')}
            value={assertion.expected}
            onChange={(event) => setAssertion({ ...assertion, expected: event.target.value })}
            disabled={assertion.operator === 'exists'}
            placeholder={t('editor.assertion_expected')}
          />
          <FormSelect
            ariaLabel={t('editor.assertion_severity')}
            value={assertion.severity}
            onChange={(severity) =>
              setAssertion({ ...assertion, severity: severity as AssertionDraft['severity'] })
            }
            options={['critical', 'warning']}
          />
        </div>
      )}
    />
  );
}

export function CheckToggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
}) {
  return (
    <label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1 hover:bg-bg-3 focus-within:bg-bg-3">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="h-4 w-4 accent-indigo"
      />
      <span>{label}</span>
    </label>
  );
}

export const KIND_OPTIONS: MonitorKind[] = [
  'http',
  'browser',
  'tcp',
  'dns',
  'icmp',
  'tls',
  'grpc',
  'heartbeat',
];

export function supportsAssertions(kind: MonitorKind) {
  return ['http', 'tcp', 'dns', 'grpc'].includes(kind);
}

export function intervalLabel(seconds: number) {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${seconds / 60}m`;
  return `${seconds / 3600}h`;
}

export function targetIsValid(draft: CheckDraft) {
  if (draft.kind === 'http') return draft.url.trim().length > 8;
  if (draft.kind === 'browser') return draft.browserSteps.trim().length > 8;
  if (draft.kind === 'heartbeat') return true;
  return draft.host.trim().length > 0;
}

import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import type { ServiceNowSettings } from '@/api/tickets';
import { FormField, FormInput, FormRadio, FormSection } from '@/shell/FormDrawer';

// A record's priority comes from its impact and urgency: what ServiceNow's default matrix reads as
// critical, high, moderate and planning.
export const SERVICENOW: ServiceNowSettings = {
  instanceUrl: 'https://',
  auth: 'basic',
  username: '',
  clientId: '',
  table: 'incident',
  assignmentGroup: '',
  callerId: '',
  category: '',
  priorities: { critical: '1/1', error: '2/1', warning: '2/2', info: '3/3' },
  resolveState: '6',
  closeCode: 'Solution provided',
  doneStates: ['6', '7', '8'],
};

const words = (text: string) =>
  text
    .split(',')
    .map((word) => word.trim())
    .filter(Boolean);

/** Where a ServiceNow connector creates records, how it signs in, and what it writes on them. */
export function ServiceNowFields({
  value,
  onChange,
  secretField,
  priorityFields,
}: {
  value: ServiceNowSettings;
  onChange: (next: ServiceNowSettings) => void;
  secretField: (key: string, label: string) => ReactNode;
  priorityFields: (priorities: Record<string, string>, onPriorities: (next: Record<string, string>) => void) => ReactNode;
}) {
  const { t } = useTranslation('alerts');

  return (
    <FormSection title={t('tickets.drawer.servicenow')}>
      <div className="grid grid-cols-[minmax(0,1fr)_160px] gap-2">
        <FormField label={t('tickets.drawer.instance_url')} required>
          <FormInput
            value={value.instanceUrl}
            onChange={(event) => onChange({ ...value, instanceUrl: event.target.value })}
            placeholder="https://acme.service-now.com"
          />
        </FormField>
        <FormField label={t('tickets.drawer.table')} hint={t('tickets.drawer.table_hint')} required>
          <FormInput
            className="font-mono"
            value={value.table}
            onChange={(event) => onChange({ ...value, table: event.target.value })}
          />
        </FormField>
      </div>
      <FormRadio
        value={value.auth}
        onChange={(auth) => onChange({ ...value, auth })}
        options={[
          { value: 'basic', label: t('tickets.drawer.auth_basic') },
          { value: 'oauth', label: t('tickets.drawer.auth_oauth'), hint: t('tickets.drawer.auth_oauth_hint') },
          { value: 'api_key', label: t('tickets.drawer.auth_api_key'), hint: t('tickets.drawer.auth_api_key_hint') },
        ]}
      />
      {value.auth === 'basic' && (
        <div className="grid grid-cols-2 gap-2">
          <FormField label={t('tickets.drawer.username')} required>
            <FormInput value={value.username} onChange={(event) => onChange({ ...value, username: event.target.value })} />
          </FormField>
          {secretField('password', t('tickets.drawer.password'))}
        </div>
      )}
      {value.auth === 'oauth' && (
        <div className="grid grid-cols-2 gap-2">
          <FormField label={t('tickets.drawer.client_id')} required>
            <FormInput value={value.clientId} onChange={(event) => onChange({ ...value, clientId: event.target.value })} />
          </FormField>
          {secretField('clientSecret', t('tickets.drawer.client_secret'))}
        </div>
      )}
      {value.auth === 'api_key' && secretField('apiKey', t('tickets.drawer.api_key'))}
      <div className="grid grid-cols-3 gap-2">
        <FormField label={t('tickets.drawer.assignment_group')} hint={t('tickets.drawer.sys_id_hint')}>
          <FormInput
            className="font-mono"
            value={value.assignmentGroup}
            onChange={(event) => onChange({ ...value, assignmentGroup: event.target.value })}
          />
        </FormField>
        <FormField label={t('tickets.drawer.caller_id')} hint={t('tickets.drawer.sys_id_hint')}>
          <FormInput
            className="font-mono"
            value={value.callerId}
            onChange={(event) => onChange({ ...value, callerId: event.target.value })}
          />
        </FormField>
        <FormField label={t('tickets.drawer.category')}>
          <FormInput
            value={value.category}
            onChange={(event) => onChange({ ...value, category: event.target.value })}
            placeholder="software"
          />
        </FormField>
      </div>
      <FormField label={t('tickets.drawer.priorities')} hint={t('tickets.drawer.priorities_hint_servicenow')}>
        {priorityFields(value.priorities, (priorities) => onChange({ ...value, priorities }))}
      </FormField>
      <div className="grid grid-cols-2 gap-2">
        <FormField label={t('tickets.drawer.resolve_state')} hint={t('tickets.drawer.resolve_state_hint')}>
          <FormInput
            className="font-mono"
            value={value.resolveState}
            onChange={(event) => onChange({ ...value, resolveState: event.target.value })}
          />
        </FormField>
        <FormField label={t('tickets.drawer.close_code')} hint={t('tickets.drawer.close_code_hint')}>
          <FormInput value={value.closeCode} onChange={(event) => onChange({ ...value, closeCode: event.target.value })} />
        </FormField>
      </div>
      <FormField label={t('tickets.drawer.done_states')} hint={t('tickets.drawer.done_states_hint_servicenow')}>
        <FormInput
          className="font-mono"
          value={value.doneStates.join(', ')}
          onChange={(event) => onChange({ ...value, doneStates: words(event.target.value) })}
          placeholder="6, 7, 8"
        />
      </FormField>
    </FormSection>
  );
}

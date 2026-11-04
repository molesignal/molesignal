import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as ticketsApi from '@/api/tickets';
import type {
  JiraSettings,
  MeegleSettings,
  MeegleSite,
  ServiceNowSettings,
  TicketConnector,
  TicketSystem,
} from '@/api/tickets';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { CopyableValue, ReadOnlyValue } from '@/routes/settings/_atoms';
import { uiLabelClass } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRadio,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { prioritiesOf, useImportance } from '@/shell/incident/Importance';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import type { Severity } from '@/types/alerting';

import { SERVICENOW, ServiceNowFields } from './ServiceNowFields';

export const CONNECTORS_KEY = ['ticket-connectors'];

const SEVERITIES: Severity[] = ['critical', 'error', 'warning', 'info'];

const JIRA: JiraSettings = {
  siteUrl: 'https://',
  deployment: 'cloud',
  email: '',
  projectKey: '',
  issueType: 'Incident',
  priorities: { critical: 'Highest', error: 'High', warning: 'Medium', info: 'Low' },
  labels: [],
  resolveTransition: '',
};

const MEEGLE: MeegleSettings = {
  site: 'feishu',
  pluginId: '',
  userKey: '',
  projectKey: '',
  workItemTypeKey: 'issue',
  priorityField: '',
  priorities: {},
  resolveTransitionId: null,
  resolveNodeId: '',
  doneStates: [],
};

const words = (text: string) =>
  text
    .split(',')
    .map((word) => word.trim())
    .filter(Boolean);

/** A connector to Jira, ServiceNow or Meegle: where tickets go, how severities map, what follows the incident, and when tickets are made on their own. */
export function TicketConnectorDrawer({
  open,
  editing,
  onClose,
}: {
  open: boolean;
  editing: TicketConnector | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const offered = prioritiesOf(levels);
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.tickets.manage' });
  const [system, setSystem] = React.useState<TicketSystem>('jira');
  const [name, setName] = React.useState('');
  const [jira, setJira] = React.useState<JiraSettings>(JIRA);
  const [meegle, setMeegle] = React.useState<MeegleSettings>(MEEGLE);
  const [servicenow, setServicenow] = React.useState<ServiceNowSettings>(SERVICENOW);
  const [secrets, setSecrets] = React.useState<Record<string, string>>({});
  const [comments, setComments] = React.useState({ acknowledged: true, resolved: true, notes: false });
  const [transitionOnResolve, setTransitionOnResolve] = React.useState(false);
  const [resolveWhenDone, setResolveWhenDone] = React.useState(false);
  const [onMajor, setOnMajor] = React.useState(false);
  const [autoEnabled, setAutoEnabled] = React.useState(false);
  const [autoSeverity, setAutoSeverity] = React.useState<Severity | ''>('critical');
  const [autoService, setAutoService] = React.useState('');
  const sites = useQuery({ queryKey: ['ticket-sites'], queryFn: ticketsApi.listSites, enabled: open, staleTime: Infinity });
  const siteAddress = sites.data?.meegle.find((site) => site.key === meegle.site)?.url;

  React.useEffect(() => {
    if (!open) return;
    setSystem(editing?.system ?? 'jira');
    setName(editing?.name ?? '');
    setJira(editing?.system === 'jira' ? { ...JIRA, ...(editing.settings as JiraSettings) } : JIRA);
    setMeegle(editing?.system === 'meegle' ? { ...MEEGLE, ...(editing.settings as MeegleSettings) } : MEEGLE);
    setServicenow(
      editing?.system === 'servicenow' ? { ...SERVICENOW, ...(editing.settings as ServiceNowSettings) } : SERVICENOW,
    );
    setSecrets({});
    setComments(editing?.comments ?? { acknowledged: true, resolved: true, notes: false });
    setTransitionOnResolve(editing?.transitionOnResolve ?? false);
    setResolveWhenDone(editing?.resolveIncidentWhenDone ?? false);
    setOnMajor(editing?.onMajorIncident ?? false);
    setAutoEnabled(editing?.auto.enabled ?? false);
    setAutoSeverity(editing?.auto.minSeverity ?? 'critical');
    setAutoService(editing?.auto.conditions.find((condition) => condition.key === 'service')?.values.join(', ') ?? '');
  }, [editing, open]);

  const isSet = (key: string) => editing?.secretsSet.includes(key) ?? false;
  const secretField = (key: string, label: string, hint?: string) => (
    <FormField label={label} {...(hint ? { hint } : {})} required={!isSet(key) && key !== 'webhookSecret'}>
      <FormInput
        type="password"
        value={secrets[key] ?? ''}
        onChange={(event) => setSecrets((current) => ({ ...current, [key]: event.target.value }))}
        placeholder={isSet(key) ? t('remediation.drawer.secret_kept') : ''}
      />
    </FormField>
  );

  const save = useMutation({
    mutationFn: () => {
      const services = words(autoService);
      const input: ticketsApi.TicketConnectorInput = {
        name: name.trim(),
        system,
        settings: system === 'jira' ? jira : system === 'servicenow' ? servicenow : meegle,
        secrets,
        comments,
        transitionOnResolve: transitionOnResolve,
        resolveIncidentWhenDone: resolveWhenDone,
        auto: {
          enabled: autoEnabled,
          minSeverity: autoSeverity || null,
          conditions: services.length > 0 ? [{ key: 'service', operator: 'in', values: services }] : [],
        },
        onMajorIncident: onMajor,
        enabled: editing?.enabled ?? true,
      };
      return editing ? ticketsApi.updateConnector(editing.id, input) : ticketsApi.createConnector(input);
    },
    onSuccess: (saved) => {
      toast.success(t('tickets.saved', { name: saved.name }));
      void queryClient.invalidateQueries({ queryKey: CONNECTORS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  // An incident's priority is looked up first, its severity when it has none or the priority is not mapped.
  const priorityFields = (priorities: Record<string, string>, onPriorities: (next: Record<string, string>) => void) => (
    <div className="flex flex-col gap-2" data-ticket-priorities>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
        {offered.map((priority) => (
          <FormField key={priority} label={priority}>
            <FormInput
              value={priorities[priority] ?? ''}
              onChange={(event) => onPriorities({ ...priorities, [priority]: event.target.value })}
            />
          </FormField>
        ))}
      </div>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {SEVERITIES.map((severity) => (
          <FormField key={severity} label={t(`history.severity.${severity}`)}>
            <FormInput
              value={priorities[severity] ?? ''}
              onChange={(event) => onPriorities({ ...priorities, [severity]: event.target.value })}
            />
          </FormField>
        ))}
      </div>
    </div>
  );

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={editing ? t('tickets.drawer.edit_title') : t('tickets.drawer.new_title')}
      subtitle={t('tickets.drawer.subtitle')}
      footer={
        <FormSubmitFooter
          formId="ticket-connector-form"
          busy={save.isPending}
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onCancel={onClose}
          submitLabel={t('tickets.drawer.save')}
        />
      }
    >
      <form
        id="ticket-connector-form"
        data-ticket-drawer
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate();
        }}
      >
        <FormSection title={t('tickets.drawer.identity')}>
          {!editing && (
            <FormRadio
              value={system}
              onChange={setSystem}
              options={[
                { value: 'jira', label: t('tickets.systems.jira') },
                { value: 'servicenow', label: t('tickets.systems.servicenow') },
                { value: 'meegle', label: t('tickets.systems.meegle'), hint: t('tickets.drawer.meegle_hint') },
              ]}
            />
          )}
          <FormField label={t('tickets.drawer.name')} required>
            <FormInput value={name} onChange={(event) => setName(event.target.value)} required />
          </FormField>
        </FormSection>

        {system === 'jira' ? (
          <FormSection title={t('tickets.drawer.jira')}>
            <FormField label={t('tickets.drawer.site_url')} required>
              <FormInput
                value={jira.siteUrl}
                onChange={(event) => setJira({ ...jira, siteUrl: event.target.value })}
                placeholder="https://acme.atlassian.net"
              />
            </FormField>
            <FormRadio
              value={jira.deployment}
              onChange={(deployment) => setJira({ ...jira, deployment })}
              options={[
                { value: 'cloud', label: t('tickets.drawer.cloud') },
                { value: 'data_center', label: t('tickets.drawer.data_center') },
              ]}
            />
            {jira.deployment === 'cloud' && (
              <FormField label={t('tickets.drawer.email')} required>
                <FormInput value={jira.email} onChange={(event) => setJira({ ...jira, email: event.target.value })} />
              </FormField>
            )}
            {secretField('apiToken', jira.deployment === 'cloud' ? t('tickets.drawer.api_token') : t('tickets.drawer.pat'))}
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('tickets.drawer.project_key')} required>
                <FormInput value={jira.projectKey} onChange={(event) => setJira({ ...jira, projectKey: event.target.value })} />
              </FormField>
              <FormField label={t('tickets.drawer.issue_type')} required>
                <FormInput value={jira.issueType} onChange={(event) => setJira({ ...jira, issueType: event.target.value })} />
              </FormField>
            </div>
            <FormField label={t('tickets.drawer.priorities')} hint={t('tickets.drawer.priorities_hint')}>
              {priorityFields(jira.priorities, (priorities) => setJira({ ...jira, priorities }))}
            </FormField>
            <FormField label={t('tickets.drawer.labels')}>
              <FormInput
                value={jira.labels.join(', ')}
                onChange={(event) => setJira({ ...jira, labels: words(event.target.value) })}
                placeholder="incident, on-call"
              />
            </FormField>
            <FormField label={t('tickets.drawer.resolve_transition')} hint={t('tickets.drawer.resolve_transition_hint')}>
              <FormInput
                value={jira.resolveTransition}
                onChange={(event) => setJira({ ...jira, resolveTransition: event.target.value })}
              />
            </FormField>
            {secretField('webhookSecret', t('tickets.drawer.webhook_secret'), t('tickets.drawer.webhook_secret_hint'))}
          </FormSection>
        ) : system === 'servicenow' ? (
          <ServiceNowFields value={servicenow} onChange={setServicenow} secretField={secretField} priorityFields={priorityFields} />
        ) : (
          <FormSection title={t('tickets.drawer.meegle')}>
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('tickets.drawer.platform')}>
                <FormSelect
                  value={meegle.site}
                  onChange={(site) => setMeegle({ ...meegle, site: site as MeegleSite })}
                  options={(['feishu', 'lark'] as const).map((site) => ({ value: site, label: t(`tickets.sites.${site}`) }))}
                />
              </FormField>
              {/* The platform's address: shown so it can be copied (to allow it through a firewall), not edited. */}
              <div data-meegle-address className="flex min-w-0 flex-col gap-1.5">
                <span className={uiLabelClass}>{t('tickets.drawer.base_url')}</span>
                {siteAddress ? (
                  <CopyableValue value={siteAddress} copyLabel={t('tickets.copy_address')} copiedLabel={t('tickets.address_copied')} />
                ) : (
                  <ReadOnlyValue value="—" muted />
                )}
              </div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('tickets.drawer.plugin_id')} required>
                <FormInput value={meegle.pluginId} onChange={(event) => setMeegle({ ...meegle, pluginId: event.target.value })} />
              </FormField>
              {secretField('pluginSecret', t('tickets.drawer.plugin_secret'))}
              <FormField label={t('tickets.drawer.user_key')} hint={t('tickets.drawer.user_key_hint')} required>
                <FormInput value={meegle.userKey} onChange={(event) => setMeegle({ ...meegle, userKey: event.target.value })} />
              </FormField>
              <FormField label={t('tickets.drawer.project_key_meegle')} required>
                <FormInput value={meegle.projectKey} onChange={(event) => setMeegle({ ...meegle, projectKey: event.target.value })} />
              </FormField>
              <FormField label={t('tickets.drawer.work_item_type')} required>
                <FormInput
                  value={meegle.workItemTypeKey}
                  onChange={(event) => setMeegle({ ...meegle, workItemTypeKey: event.target.value })}
                />
              </FormField>
              <FormField label={t('tickets.drawer.priority_field')}>
                <FormInput
                  value={meegle.priorityField}
                  onChange={(event) => setMeegle({ ...meegle, priorityField: event.target.value })}
                  placeholder="priority"
                />
              </FormField>
            </div>
            {meegle.priorityField.trim() && (
              <FormField label={t('tickets.drawer.priorities')} hint={t('tickets.drawer.priorities_hint_meegle')}>
                {priorityFields(meegle.priorities, (priorities) => setMeegle({ ...meegle, priorities }))}
              </FormField>
            )}
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('tickets.drawer.resolve_transition_id')} hint={t('tickets.drawer.resolve_transition_id_hint')}>
                <FormInput
                  type="number"
                  value={meegle.resolveTransitionId ?? ''}
                  onChange={(event) =>
                    setMeegle({ ...meegle, resolveTransitionId: event.target.value ? Number(event.target.value) : null })
                  }
                />
              </FormField>
              <FormField label={t('tickets.drawer.resolve_node_id')} hint={t('tickets.drawer.resolve_node_id_hint')}>
                <FormInput value={meegle.resolveNodeId} onChange={(event) => setMeegle({ ...meegle, resolveNodeId: event.target.value })} />
              </FormField>
            </div>
            <FormField label={t('tickets.drawer.done_states')} hint={t('tickets.drawer.done_states_hint')}>
              <FormInput
                value={meegle.doneStates.join(', ')}
                onChange={(event) => setMeegle({ ...meegle, doneStates: words(event.target.value) })}
                placeholder="CLOSED, RESOLVED"
              />
            </FormField>
          </FormSection>
        )}

        <FormSection title={t('tickets.drawer.follow')}>
          {(
            [
              ['acknowledged', comments.acknowledged],
              ['resolved', comments.resolved],
              ['notes', comments.notes],
            ] as const
          ).map(([key, checked]) => (
            <label key={key} className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={checked} onCheckedChange={(next) => setComments((current) => ({ ...current, [key]: next === true }))} />
              <span>{t(`tickets.drawer.comments.${key}`)}</span>
            </label>
          ))}
          {(
            [
              ['transition_on_resolve', transitionOnResolve, setTransitionOnResolve],
              ['resolve_when_done', resolveWhenDone, setResolveWhenDone],
              ['on_major', onMajor, setOnMajor],
            ] as const
          ).map(([key, checked, setChecked]) => (
            <label key={key} className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={checked} onCheckedChange={(next) => setChecked(next === true)} />
              <span>{t(`tickets.drawer.${key}`)}</span>
            </label>
          ))}
        </FormSection>

        <FormSection title={t('tickets.drawer.auto')} description={t('tickets.drawer.auto_hint')}>
          <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
            <Checkbox checked={autoEnabled} onCheckedChange={(next) => setAutoEnabled(next === true)} />
            <span>{t('tickets.drawer.auto_enabled')}</span>
          </label>
          {autoEnabled && (
            <div className="grid grid-cols-2 gap-2">
              <FormField label={t('tickets.drawer.auto_severity')}>
                <FormSelect
                  value={autoSeverity}
                  onChange={(value) => setAutoSeverity(value as Severity | '')}
                  options={[
                    { value: '', label: t('routing.drawer.any_severity') },
                    ...SEVERITIES.map((severity) => ({ value: severity, label: t(`history.severity.${severity}`) })),
                  ]}
                />
              </FormField>
              <FormField label={t('tickets.drawer.auto_services')} hint={t('tickets.drawer.auto_services_hint')}>
                <FormInput value={autoService} onChange={(event) => setAutoService(event.target.value)} placeholder="checkout, payments" />
              </FormField>
            </div>
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as escalationsApi from '@/api/escalations';
import * as incidentsApi from '@/api/incidents';
import type { NewIncidentAssignment } from '@/api/incidents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRadio,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { prioritiesOf, priorityLabel, urgenciesOf, urgencyLabel, useImportance } from '@/shell/incident/Importance';
import { refreshIncidents } from '@/shell/incident/refresh';
import { useTeams } from '@/shell/incident/Routing';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { Priority, Severity, Urgency } from '@/types/alerting';

import { SEVERITIES_WORST_FIRST } from './model';
import { labelsFromText, valuesFromText } from '../routing/model';

type AssignKind = NewIncidentAssignment['kind'];

const ASSIGN_KINDS: readonly AssignKind[] = ['auto', 'team', 'policy', 'users'];

/** At most this many people are named, as the server takes them. */
const MAX_PEOPLE = 10;

/** The longest title the server keeps. */
const MAX_TITLE = 200;

/**
 * An incident opened by hand, for what no rule caught: a customer's report, something someone
 * saw. It says what happened and how bad, and who should take it; it then pages like any other.
 */
export function IncidentCreateDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const urgencies = urgenciesOf(levels);
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'alerts.incidents.create' });
  const users = useUsers();
  const teams = useTeams();
  const policies = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list(), enabled: open });
  const [title, setTitle] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [severity, setSeverity] = React.useState<Severity>('error');
  const [priority, setPriority] = React.useState<Priority | ''>('');
  const [urgency, setUrgency] = React.useState<Urgency | ''>('');
  const [assignKind, setAssignKind] = React.useState<AssignKind>('auto');
  const [teamId, setTeamId] = React.useState('');
  const [policyId, setPolicyId] = React.useState('');
  const [people, setPeople] = React.useState<string[]>([]);
  const [services, setServices] = React.useState('');
  const [labels, setLabels] = React.useState('');
  const [major, setMajor] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    setTitle('');
    setDescription('');
    setSeverity('error');
    setPriority('');
    setUrgency('');
    setAssignKind('auto');
    setTeamId('');
    setPolicyId('');
    setPeople([]);
    setServices('');
    setLabels('');
    setMajor(false);
  }, [open]);

  const assignment = (): NewIncidentAssignment => {
    if (assignKind === 'team') return { kind: 'team', teamId: teamId };
    if (assignKind === 'policy') return { kind: 'policy', escalationPolicyId: policyId };
    if (assignKind === 'users') return { kind: 'users', userIds: people };
    return { kind: 'auto' };
  };

  const create = useMutation({
    mutationFn: () =>
      incidentsApi.create({
        title: title.trim(),
        ...(description.trim() ? { description: description.trim() } : {}),
        severity,
        ...(priority ? { priority } : {}),
        ...(urgency ? { urgency } : {}),
        assign: assignment(),
        services: valuesFromText('in', services),
        labels: labelsFromText(labels),
        declareMajor: major,
      }),
    onSuccess: async (incident) => {
      toast.success(t('create_incident.created'));
      await refreshIncidents(queryClient, incident.id);
      onClose();
      navigate(`/alerts/incidents/${encodeURIComponent(incident.id)}`);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const target =
    assignKind === 'team' ? teamId : assignKind === 'policy' ? policyId : assignKind === 'users' ? people[0] ?? '' : 'auto';
  const invalid = !title.trim() || !target;
  const teamRows = Array.isArray(teams.data) ? teams.data : [];

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('create_incident.title')}
      subtitle={t('create_incident.subtitle')}
      footer={
        <FormSubmitFooter
          formId="incident-create-form"
          busy={create.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('create_incident.submit')}
        />
      }
    >
      <form
        id="incident-create-form"
        data-incident-create
        onSubmit={(event) => {
          event.preventDefault();
          if (!access.allowed || invalid) return;
          create.mutate();
        }}
      >
        <FormSection title={t('create_incident.sections.what')}>
          <FormField label={t('create_incident.fields.title')} required>
            <FormInput
              value={title}
              maxLength={MAX_TITLE}
              onChange={(event) => setTitle(event.target.value)}
              placeholder={t('create_incident.placeholders.title')}
              required
            />
          </FormField>
          <FormField label={t('create_incident.fields.description')} hint={t('create_incident.hints.description')}>
            <FormTextarea value={description} rows={5} onChange={(event) => setDescription(event.target.value)} />
          </FormField>
          <div className="grid gap-3 sm:grid-cols-3">
            <FormField label={t('create_incident.fields.severity')}>
              <FormSelect
                ariaLabel={t('create_incident.fields.severity')}
                value={severity}
                onChange={(next) => setSeverity(next as Severity)}
                options={SEVERITIES_WORST_FIRST.map((value) => ({ value, label: t(`severity.${value}`) }))}
              />
            </FormField>
            <FormField label={t('create_incident.fields.priority')}>
              <FormSelect
                ariaLabel={t('create_incident.fields.priority')}
                value={priority}
                onChange={(next) => setPriority(next as Priority | '')}
                options={[
                  { value: '', label: t('importance.without_priority') },
                  ...priorities.map((value) => ({ value, label: priorityLabel(levels, value) })),
                ]}
              />
            </FormField>
            <FormField label={t('create_incident.fields.urgency')}>
              <FormSelect
                ariaLabel={t('create_incident.fields.urgency')}
                value={urgency}
                onChange={(next) => setUrgency(next as Urgency | '')}
                options={[
                  { value: '', label: t('workbench.urgency_by_policy') },
                  ...urgencies.map((value) => ({ value, label: urgencyLabel(levels, value, t) })),
                ]}
              />
            </FormField>
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <FormField label={t('create_incident.fields.services')} hint={t('create_incident.hints.services')}>
              <FormInput value={services} onChange={(event) => setServices(event.target.value)} placeholder="checkout, payments" />
            </FormField>
            <FormField label={t('create_incident.fields.labels')} hint={t('create_incident.hints.labels')}>
              <FormTextarea
                value={labels}
                rows={2}
                spellCheck={false}
                className="font-mono text-xs"
                placeholder={'env=prod\nregion=cn-north'}
                onChange={(event) => setLabels(event.target.value)}
              />
            </FormField>
          </div>
        </FormSection>

        <FormSection title={t('create_incident.sections.who')} className="mb-0">
          <div data-incident-assign={assignKind}>
            <FormRadio<AssignKind>
              value={assignKind}
              onChange={setAssignKind}
              options={ASSIGN_KINDS.map((kind) => ({
                value: kind,
                label: t(`create_incident.assign.${kind}`),
                hint: t(`create_incident.assign_hints.${kind}`),
              }))}
            />
          </div>
          {assignKind === 'team' && (
            <FormField label={t('create_incident.fields.team')} required>
              <FormSelect
                ariaLabel={t('create_incident.fields.team')}
                value={teamId}
                onChange={setTeamId}
                placeholder={t('create_incident.placeholders.choose')}
                options={teamRows.map((team) => ({ value: team.id, label: team.name }))}
              />
            </FormField>
          )}
          {assignKind === 'policy' && (
            <FormField label={t('create_incident.fields.policy')} required>
              <FormSelect
                ariaLabel={t('create_incident.fields.policy')}
                value={policyId}
                onChange={setPolicyId}
                placeholder={t('create_incident.placeholders.choose')}
                options={(policies.data ?? []).map((policy) => ({ value: policy.id, label: policy.name }))}
              />
            </FormField>
          )}
          {assignKind === 'users' && (
            <FormGroup label={t('create_incident.fields.people')} hint={t('create_incident.hints.people', { count: MAX_PEOPLE })}>
              <FormChecklist
                className="max-h-64 overflow-y-auto"
                selected={people}
                onChange={(next) => setPeople(next.slice(0, MAX_PEOPLE))}
                options={users.users.map((user) => ({ value: user.id, label: user.name }))}
              />
            </FormGroup>
          )}
          <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
            <Checkbox checked={major} onCheckedChange={(next) => setMajor(next === true)} data-declare-major />
            <span>{t('create_incident.declare_major')}</span>
          </label>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

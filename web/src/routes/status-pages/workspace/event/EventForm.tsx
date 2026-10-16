import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import type {
  IncidentImpact,
  PublicIncidentStatus,
  StatusPageIncident,
  StatusPageIncidentKind,
} from '@/api/statusPages';
import { useActionAccess } from '@/product/actionAccess';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormChecklist,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';

import {
  INCIDENT_IMPACTS,
  INCIDENT_STATUSES,
  impactLabel,
  incidentStatusLabel,
  namedComponents,
} from '../../model';
import { useStatusPageWorkspace } from '../Layout';
import type { EventDraft } from './draft';
import { PublishOptions } from './PublishOptions';

/** The fields of an event that is still a draft, a new one, or maintenance yet to start. */
export function EventForm({
  kind,
  event,
  draft,
  setDraft,
  disabled,
  publishOptions,
}: {
  kind: StatusPageIncidentKind;
  event: StatusPageIncident | null;
  draft: EventDraft;
  setDraft: React.Dispatch<React.SetStateAction<EventDraft>>;
  disabled: boolean;
  /** Whether the form's main action publishes, so who is told and what is promised are asked. */
  publishOptions: boolean;
}) {
  const { t } = useTranslation('status-pages');
  const { snapshot } = useStatusPageWorkspace();
  const incidentReadAccess = useActionAccess({ permission: 'alerts.read' });
  const sourceIncidents = useQuery({
    queryKey: ['status-pages', 'source-incidents'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled: kind === 'incident' && incidentReadAccess.allowed,
  });
  const set = <K extends keyof EventDraft>(key: K, value: EventDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  const statuses = kind === 'incident' ? INCIDENT_STATUSES.slice(0, -1) : ['scheduled'] as const;

  return (
    <>
      <FormSection title={t('forms.event.details')}>
        <FormField label={t('fields.title')} required>
          <FormInput
            value={draft.title}
            maxLength={200}
            disabled={disabled}
            onChange={(change) => set('title', change.currentTarget.value)}
          />
        </FormField>
        {kind === 'incident' && incidentReadAccess.allowed && (
          <FormField label={t('fields.source_incident')} hint={t('forms.incident.source_hint')}>
            <FormSelect
              value={draft.sourceIncidentId}
              disabled={disabled || sourceIncidents.isLoading}
              onChange={(value) => set('sourceIncidentId', value)}
              options={[
                { value: '', label: t('forms.incident.source_placeholder') },
                ...(sourceIncidents.data ?? []).map((incident) => ({
                  value: incident.id,
                  label: incident.summary,
                })),
              ]}
            />
          </FormField>
        )}
        <FormRow className="grid-cols-1 sm:grid-cols-2">
          {kind === 'incident' ? (
            <FormField label={t('fields.impact')}>
              <FormSelect
                value={draft.impact}
                disabled={disabled}
                onChange={(value) => set('impact', value as IncidentImpact)}
                options={INCIDENT_IMPACTS.map((impact) => ({ value: impact, label: impactLabel(t, impact) }))}
              />
            </FormField>
          ) : (
            <FormField label={t('fields.starts_at')} required>
              <DateTimePicker
                value={draft.startedAt}
                disabled={disabled}
                onChange={(value) => set('startedAt', value)}
              />
            </FormField>
          )}
          <FormField label={t('fields.incident_status')}>
            <FormSelect
              value={draft.status}
              disabled={disabled || event?.status === 'scheduled'}
              onChange={(value) => set('status', value as PublicIncidentStatus)}
              options={statuses.map((status) => ({ value: status, label: incidentStatusLabel(t, status, kind) }))}
            />
          </FormField>
        </FormRow>
      </FormSection>
      <FormSection title={t('forms.incident.affected_components')}>
        <FormChecklist
          options={namedComponents(snapshot.components)
            .filter((component) => component.lifecycle === 'active' || draft.componentIds.includes(component.id))
            .map((component) => ({
              value: component.id,
              label: component.name,
              hint: component.lifecycle === 'archived' ? t('lifecycle.archived') : component.description,
            }))}
          selected={draft.componentIds}
          onChange={(value) => set('componentIds', value)}
          disabled={disabled}
        />
      </FormSection>
      <FormSection title={t('forms.event.customer_update')}>
        <FormField label={t('fields.message')} required>
          <FormTextarea
            value={draft.message}
            maxLength={4_000}
            disabled={disabled}
            onChange={(change) => set('message', change.currentTarget.value)}
            placeholder={t('forms.incident.message_placeholder')}
          />
        </FormField>
        {publishOptions && (
          <PublishOptions
            kind={kind}
            status={draft.status}
            nextUpdateMinutes={draft.nextUpdateMinutes}
            onNextUpdateChange={(minutes) => set('nextUpdateMinutes', minutes)}
            notify={draft.notify}
            onNotifyChange={(notify) => set('notify', notify)}
            disabled={disabled}
          />
        )}
      </FormSection>
    </>
  );
}

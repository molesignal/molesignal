import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Send } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import * as statusPagesApi from '@/api/statusPages';
import type {
  IncidentImpact,
  PublicIncidentStatus,
  StatusPageIncident,
  StatusPageIncidentInput,
  StatusPageIncidentKind,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  INCIDENT_IMPACTS,
  INCIDENT_STATUSES,
  INCIDENT_TONE,
  affectedComponentNames,
  formatMicros,
  impactLabel,
  incidentStatusLabel,
} from '../model';
import { primaryEventSubmitAction } from './eventSubmission';
import { useStatusPageWorkspace } from './Layout';

type SubmitMode = 'draft' | 'publish' | 'reschedule';

export function StatusPageEventDrawer({
  kind,
  eventId,
  onClose,
}: {
  kind: StatusPageIncidentKind;
  eventId: string | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const { pageId, snapshot, manageAccess, refresh } = useStatusPageWorkspace();
  const publishAccess = useActionAccess({ permission: 'status_pages.publish' });
  const isNew = eventId === 'new';
  const eventQuery = useQuery({
    queryKey: ['status-pages', pageId, 'event', eventId],
    queryFn: () => statusPagesApi.getEvent(pageId, eventId ?? ''),
    enabled: Boolean(eventId) && !isNew,
  });
  const event = isNew ? null : eventQuery.data ?? null;
  const [draft, setDraft] = React.useState(() => initialDraft(kind, null));
  const [updateStatus, setUpdateStatus] = React.useState<PublicIncidentStatus>('investigating');
  const [updateMessage, setUpdateMessage] = React.useState('');

  React.useEffect(() => {
    if (isNew || event) setDraft(initialDraft(kind, event));
    if (event) {
      setUpdateStatus(nextStatuses(event)[0] ?? event.status);
      setUpdateMessage('');
    }
  }, [event, isNew, kind]);

  const eventMutation = useMutation({
    mutationFn: async (mode: SubmitMode) => {
      const input = eventInput(kind, draft, mode === 'draft' ? 'draft' : 'published');
      if (!event) return statusPagesApi.createIncident(pageId, input);
      if (event.publication_state === 'draft') {
        return mode === 'publish'
          ? statusPagesApi.publishDraft(pageId, event.id, input)
          : statusPagesApi.updateEvent(pageId, event.id, input);
      }
      return statusPagesApi.updateEvent(pageId, event.id, input);
    },
    onSuccess: async (_, mode) => {
      toast.success(t(mode === 'draft' ? 'toast.draft_saved' : 'toast.event_published'));
      await invalidate();
      onClose();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const updateMutation = useMutation({
    mutationFn: () =>
      statusPagesApi.appendIncidentUpdate(pageId, event?.id ?? '', {
        status: updateStatus,
        message: updateMessage.trim(),
      }),
    onSuccess: async () => {
      toast.success(t('toast.update_published'));
      await invalidate();
      onClose();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const invalidate = async () => {
    await Promise.all([
      refresh(),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'event'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'events'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'history'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'automation'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', 'automation', 'pending'] }),
    ]);
  };

  const pageActive = snapshot.page.lifecycle === 'active';
  const canManage = manageAccess.allowed && pageActive;
  const canPublish = publishAccess.allowed && pageActive;
  const editable =
    isNew ||
    event?.publication_state === 'draft' ||
    (event?.kind === 'maintenance' && event.status === 'scheduled');
  const canEditForm = isNew
    ? canManage
    : event?.publication_state === 'draft'
      ? canManage || canPublish
      : canManage;
  const publicationInvalid =
    !draft.title.trim() || !draft.message.trim() || draft.componentIds.length === 0;
  const draftInvalid = draft.title.length > 200 || draft.message.length > 4_000;
  const startedAtInvalid =
    kind === 'maintenance' && !Number.isFinite(new Date(draft.startedAt).getTime());
  const primaryAction = primaryEventSubmitAction(event);
  const canSubmitPrimary = primaryAction.mode === 'publish'
    ? event
      ? canPublish
      : canManage
    : canManage;

  return (
    <FormDrawer
      open={Boolean(eventId)}
      onOpenChange={(open) => !open && onClose()}
      title={
        isNew
          ? t(kind === 'incident' ? 'forms.event.new_incident' : 'forms.event.new_maintenance')
          : event?.title || t('forms.event.loading')
      }
      subtitle={t(`forms.event.${kind}_subtitle`)}
      footer={
        editable && canEditForm ? (
          <>
            {(isNew || event?.publication_state === 'draft') && canManage && (
              <ChromeButton
                disabled={draftInvalid || eventMutation.isPending}
                onClick={() => eventMutation.mutate('draft')}
              >
                {t('actions.save_draft')}
              </ChromeButton>
            )}
            <ChromeButton
              variant="primary"
              disabled={
                !canSubmitPrimary
                || publicationInvalid
                || startedAtInvalid
                || eventMutation.isPending
              }
              onClick={() => eventMutation.mutate(primaryAction.mode)}
            >
              <Send className="h-3.5 w-3.5" />
              {t(`actions.${primaryAction.label}`)}
            </ChromeButton>
          </>
        ) : undefined
      }
    >
      {eventQuery.isLoading && !isNew ? (
        <div className="py-16 text-center text-sm text-tx-3">{t('states.loading')}</div>
      ) : editable ? (
        <EventForm
          kind={kind}
          event={event}
          draft={draft}
          setDraft={setDraft}
          disabled={!canEditForm}
        />
      ) : event ? (
        <EventDetail event={event} />
      ) : (
        <div className="py-16 text-center text-sm text-red-soft">{t('states.event_unavailable')}</div>
      )}

      {event && !editable && !isTerminal(event) && canPublish && (
        <FormSection title={t('forms.update.title')} description={t('forms.update.hint')}>
          <FormField label={t('fields.incident_status')}>
            <FormSelect
              value={updateStatus}
              onChange={(value) => setUpdateStatus(value as PublicIncidentStatus)}
              options={nextStatuses(event).map((status) => ({
                value: status,
                label: incidentStatusLabel(t, status),
              }))}
            />
          </FormField>
          <FormField label={t('fields.message')} required>
            <FormTextarea
              value={updateMessage}
              maxLength={4_000}
              onChange={(change) => setUpdateMessage(change.currentTarget.value)}
            />
          </FormField>
          <ChromeButton
            variant="primary"
            disabled={!updateMessage.trim() || updateMutation.isPending}
            onClick={() => updateMutation.mutate()}
          >
            {t('actions.publish_update')}
          </ChromeButton>
        </FormSection>
      )}
    </FormDrawer>
  );
}

interface EventDraft {
  title: string;
  sourceIncidentId: string;
  impact: IncidentImpact;
  status: PublicIncidentStatus;
  message: string;
  componentIds: string[];
  startedAt: string;
}

function EventForm({
  kind,
  event,
  draft,
  setDraft,
  disabled,
}: {
  kind: StatusPageIncidentKind;
  event: StatusPageIncident | null;
  draft: EventDraft;
  setDraft: React.Dispatch<React.SetStateAction<EventDraft>>;
  disabled: boolean;
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
              options={statuses.map((status) => ({ value: status, label: incidentStatusLabel(t, status) }))}
            />
          </FormField>
        </FormRow>
      </FormSection>
      <FormSection title={t('forms.incident.affected_components')}>
        <FormChecklist
          options={snapshot.components
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
      </FormSection>
    </>
  );
}

function EventDetail({ event }: { event: StatusPageIncident }) {
  const { t, i18n } = useTranslation('status-pages');
  const { snapshot } = useStatusPageWorkspace();
  const components = affectedComponentNames(event, snapshot.components);
  return (
    <div className="space-y-7">
      <dl className="grid grid-cols-2 gap-4 rounded-lg border border-bd-0 bg-bg-2 p-4 text-sm">
        <Detail label={t('fields.incident_status')}>
          <Pill tone={INCIDENT_TONE[event.status]}>{incidentStatusLabel(t, event.status)}</Pill>
        </Detail>
        <Detail label={t('fields.impact')}>{impactLabel(t, event.impact)}</Detail>
        <Detail label={t('fields.starts_at')}>
          {formatMicros(event.started_at, snapshot.page.timezone, i18n.language)}
        </Detail>
        <Detail label={t('fields.affected_components')}>
          {components.join(', ') || t('values.none')}
        </Detail>
      </dl>
      <section>
        <h3 className="text-sm font-display-strong text-tx-0">{t('forms.event.timeline')}</h3>
        <div className="mt-3 border-l border-bd-1 pl-4">
          {event.updates.map((update) => (
            <article key={update.id} className="relative pb-6 last:pb-0">
              <span className="absolute -left-[19px] top-1.5 h-2 w-2 rounded-full bg-indigo" />
              <div className="flex items-center gap-2">
                <span className="text-xs font-strong text-tx-1">
                  {incidentStatusLabel(t, update.status)}
                </span>
                <time className="text-xs tabular-nums text-tx-3">
                  {formatMicros(update.created_at, snapshot.page.timezone, i18n.language)}
                </time>
              </div>
              <p className="mt-1.5 whitespace-pre-wrap text-sm leading-6 text-tx-2">{update.message}</p>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}

function Detail({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt className="text-xs text-tx-3">{label}</dt>
      <dd className="mt-1 text-tx-1">{children}</dd>
    </div>
  );
}

function initialDraft(kind: StatusPageIncidentKind, event: StatusPageIncident | null): EventDraft {
  return {
    title: event?.title ?? '',
    sourceIncidentId: event?.source_incident_id ?? '',
    impact: event?.impact ?? (kind === 'maintenance' ? 'maintenance' : 'major'),
    status: event?.status ?? (kind === 'maintenance' ? 'scheduled' : 'investigating'),
    message: event?.draft_message ?? '',
    componentIds: event?.component_ids ?? [],
    startedAt: toLocalDateTime(event?.started_at ?? (Date.now() + 60 * 60_000) * 1_000),
  };
}

function eventInput(
  kind: StatusPageIncidentKind,
  draft: EventDraft,
  publicationState: 'draft' | 'published',
): StatusPageIncidentInput {
  return {
    source_incident_id: kind === 'incident' ? draft.sourceIncidentId || null : null,
    kind,
    title: draft.title.trim(),
    impact: kind === 'maintenance' ? 'maintenance' : draft.impact,
    status: draft.status,
    publication_state: publicationState,
    message: draft.message.trim() || null,
    component_ids: draft.componentIds,
    ...(kind === 'maintenance' ? { started_at: new Date(draft.startedAt).getTime() * 1_000 } : {}),
  };
}

function nextStatuses(event: StatusPageIncident): PublicIncidentStatus[] {
  if (event.kind === 'maintenance') {
    if (event.status === 'scheduled') return ['scheduled', 'in_progress', 'cancelled'];
    if (event.status === 'in_progress') return ['in_progress', 'completed', 'cancelled'];
    return [];
  }
  const index = INCIDENT_STATUSES.indexOf(event.status);
  if (event.status === 'monitoring') return ['monitoring', 'in_progress', 'resolved'];
  return index >= 0 ? INCIDENT_STATUSES.slice(index) : [];
}

function isTerminal(event: StatusPageIncident): boolean {
  return event.kind === 'incident'
    ? event.status === 'resolved'
    : event.status === 'completed' || event.status === 'cancelled';
}

function toLocalDateTime(micros: number): string {
  const date = new Date(Math.floor(micros / 1_000));
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

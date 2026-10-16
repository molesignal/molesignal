import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { PublicIncidentStatus, StatusPageIncident } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormSection, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { nextStatuses, nextUpdateAt } from './draft';
import { PublishOptions } from './PublishOptions';
import { incidentStatusLabel } from '../../model';

/** The next word on an event that is going on: its status, what to say, what to promise. */
export function UpdateForm({
  pageId,
  event,
  onPublished,
}: {
  pageId: string;
  event: StatusPageIncident;
  onPublished: () => Promise<void>;
}) {
  const { t } = useTranslation('status-pages');
  const [status, setStatus] = React.useState<PublicIncidentStatus>(
    () => nextStatuses(event)[0] ?? event.status,
  );
  const [message, setMessage] = React.useState('');
  const [nextUpdateMinutes, setNextUpdateMinutes] = React.useState<number | null>(null);
  const [notify, setNotify] = React.useState(true);

  React.useEffect(() => {
    setStatus(nextStatuses(event)[0] ?? event.status);
    setMessage('');
    setNextUpdateMinutes(null);
    setNotify(true);
  }, [event]);

  const updateMutation = useMutation({
    mutationFn: () =>
      statusPagesApi.appendIncidentUpdate(pageId, event.id, {
        status,
        message: message.trim(),
        next_update_at: nextUpdateAt(event.kind, status, nextUpdateMinutes),
        notify,
      }),
    onSuccess: async () => {
      toast.success(t(notify ? 'toast.update_published' : 'toast.update_published_quietly'));
      await onPublished();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  return (
    <FormSection title={t('forms.update.title')} description={t('forms.update.hint')}>
      <FormField label={t('fields.incident_status')}>
        <FormSelect
          value={status}
          onChange={(value) => setStatus(value as PublicIncidentStatus)}
          options={nextStatuses(event).map((option) => ({
            value: option,
            label: incidentStatusLabel(t, option, event.kind),
          }))}
        />
      </FormField>
      <FormField label={t('fields.message')} required>
        <FormTextarea
          value={message}
          maxLength={4_000}
          onChange={(change) => setMessage(change.currentTarget.value)}
        />
      </FormField>
      <PublishOptions
        kind={event.kind}
        status={status}
        nextUpdateMinutes={nextUpdateMinutes}
        onNextUpdateChange={setNextUpdateMinutes}
        notify={notify}
        onNotifyChange={setNotify}
      />
      <ChromeButton
        variant="primary"
        disabled={!message.trim() || updateMutation.isPending}
        onClick={() => updateMutation.mutate()}
      >
        {t('actions.publish_update')}
      </ChromeButton>
    </FormSection>
  );
}

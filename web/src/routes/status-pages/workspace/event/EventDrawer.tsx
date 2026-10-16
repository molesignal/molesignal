import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Send } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageIncidentKind } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { primaryEventSubmitAction } from '../eventSubmission';
import { useStatusPageWorkspace } from '../Layout';
import { eventInput, initialDraft, isTerminal } from './draft';
import { EventDetail } from './EventDetail';
import { EventForm } from './EventForm';
import { PostmortemSection } from './Postmortem';
import { UpdateForm } from './UpdateForm';

type SubmitMode = 'draft' | 'publish' | 'reschedule';

/**
 * One incident or piece of maintenance of the page: written and published
 * here, then updated while it goes on, and for a resolved incident, its
 * postmortem.
 */
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

  React.useEffect(() => {
    if (isNew || event) setDraft(initialDraft(kind, event));
  }, [event, isNew, kind]);

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
  // A postmortem is written once the incident is public, and shown here once it is
  // resolved or a draft was started earlier.
  const showPostmortem = Boolean(
    event
      && event.kind === 'incident'
      && event.publication_state === 'published'
      && (event.status === 'resolved' || event.postmortem),
  );

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
          publishOptions={canSubmitPrimary}
        />
      ) : event ? (
        <EventDetail event={event} />
      ) : (
        <div className="py-16 text-center text-sm text-red-soft">{t('states.event_unavailable')}</div>
      )}

      {event && !editable && !isTerminal(event) && canPublish && (
        <UpdateForm
          pageId={pageId}
          event={event}
          onPublished={async () => {
            await invalidate();
            onClose();
          }}
        />
      )}

      {event && showPostmortem && (canManage || canPublish) && (
        <PostmortemSection
          pageId={pageId}
          event={event}
          timezone={snapshot.page.timezone}
          canManage={canManage}
          canPublish={canPublish}
          onChanged={invalidate}
        />
      )}
    </FormDrawer>
  );
}

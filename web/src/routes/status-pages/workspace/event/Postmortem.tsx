import { useMutation } from '@tanstack/react-query';
import { Send } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageIncident } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { MarkdownMessage } from '@/routes/agent/markdown';
import { ChromeButton } from '@/shell/chrome';
import { FormSection, FormTextarea } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { formatMicros } from '../../model';
import { NotifySwitch } from '../NotifySwitch';

type PostmortemAction = 'save' | 'publish' | 'unpublish' | 'delete';

/**
 * What the customers of an incident are told about it afterwards: why it
 * happened and what changes because of it. Written in Markdown, kept as a
 * draft until the incident is resolved and someone publishes it.
 */
export function PostmortemSection({
  pageId,
  event,
  timezone,
  canManage,
  canPublish,
  onChanged,
}: {
  pageId: string;
  event: StatusPageIncident;
  timezone: string;
  canManage: boolean;
  canPublish: boolean;
  onChanged: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation('status-pages');
  const postmortem = event.postmortem ?? null;
  const published = postmortem?.publication_state === 'published';
  const resolved = event.status === 'resolved';
  const [body, setBody] = React.useState(postmortem?.body ?? '');
  const [view, setView] = React.useState<'write' | 'preview'>('write');
  // A first publication is news; a revision usually corrects wording.
  const [notify, setNotify] = React.useState(!published);

  React.useEffect(() => {
    setBody(event.postmortem?.body ?? '');
    setNotify(event.postmortem?.publication_state !== 'published');
  }, [event.id, event.postmortem?.body, event.postmortem?.publication_state]);

  const trimmed = body.trim();
  const changed = trimmed !== (postmortem?.body ?? '').trim();
  const tooLong = Array.from(trimmed).length > statusPagesApi.MAX_POSTMORTEM_CHARS;

  const mutation = useMutation({
    mutationFn: (action: PostmortemAction) => {
      if (action === 'save') return statusPagesApi.savePostmortemDraft(pageId, event.id, trimmed);
      if (action === 'publish') return statusPagesApi.publishPostmortem(pageId, event.id, trimmed, notify);
      if (action === 'unpublish') return statusPagesApi.unpublishPostmortem(pageId, event.id);
      return statusPagesApi.deletePostmortemDraft(pageId, event.id);
    },
    onSuccess: async (_, action) => {
      toast.success(t(`toast.postmortem_${action === 'publish' && published ? 'revised' : action}`));
      await onChanged();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const time = (micros: number) => formatMicros(micros, timezone, i18n.language);
  const state = !postmortem
    ? t('forms.postmortem.none')
    : published
      ? [
          t('forms.postmortem.published_at', { time: time(postmortem.published_at ?? postmortem.updated_at) }),
          postmortem.revised_at && postmortem.revised_at !== postmortem.published_at
            ? t('forms.postmortem.revised_at', { time: time(postmortem.revised_at) })
            : null,
        ].filter(Boolean).join(' · ')
      : t('forms.postmortem.draft_saved_at', { time: time(postmortem.updated_at) });

  return (
    <FormSection title={t('forms.postmortem.title')} description={state}>
      <ViewTabs
        label={t('forms.postmortem.view_label')}
        value={view}
        onValueChange={(next) => setView(next === 'preview' ? 'preview' : 'write')}
        items={[
          { key: 'write', label: t('forms.postmortem.write') },
          { key: 'preview', label: t('forms.postmortem.preview') },
        ]}
      />
      {view === 'write' ? (
        <FormTextarea
          aria-label={t('forms.postmortem.title')}
          value={body}
          disabled={!canManage && !canPublish}
          rows={12}
          onChange={(change) => setBody(change.currentTarget.value)}
          placeholder={t('forms.postmortem.placeholder')}
        />
      ) : (
        <div className="min-h-[120px] rounded-md bg-bg-2 px-4 py-3">
          {trimmed ? (
            <MarkdownMessage content={trimmed} className="text-sm" />
          ) : (
            <p className="text-sm text-tx-3">{t('forms.postmortem.empty_preview')}</p>
          )}
        </div>
      )}
      {tooLong && (
        <p role="alert" className="text-xs text-red-soft">{t('forms.postmortem.too_long')}</p>
      )}
      {canPublish && (
        <NotifySwitch
          label={t('fields.notify_subscribers')}
          hint={t(published ? 'forms.postmortem.notify_revision_hint' : 'forms.postmortem.notify_hint')}
          checked={notify}
          onCheckedChange={setNotify}
        />
      )}
      <div className="flex flex-wrap items-center gap-2">
        {canPublish && (
          <ChromeButton
            variant="primary"
            disabled={!resolved || !trimmed || tooLong || (published && !changed) || mutation.isPending}
            disabledReason={resolved ? undefined : t('forms.postmortem.publish_after_resolution')}
            onClick={() => mutation.mutate('publish')}
          >
            <Send className="h-3.5 w-3.5" />
            {t(published ? 'actions.publish_revision' : 'actions.publish_postmortem')}
          </ChromeButton>
        )}
        {canManage && !published && (
          <ChromeButton
            disabled={!trimmed || tooLong || !changed || mutation.isPending}
            onClick={() => mutation.mutate('save')}
          >
            {t('actions.save_draft')}
          </ChromeButton>
        )}
        {canPublish && published && (
          <ChromeButton disabled={mutation.isPending} onClick={() => mutation.mutate('unpublish')}>
            {t('actions.unpublish_postmortem')}
          </ChromeButton>
        )}
        {canManage && postmortem && !published && (
          <ChromeButton
            className="enabled:hover:text-red-soft"
            disabled={mutation.isPending}
            onClick={() => mutation.mutate('delete')}
          >
            {t('actions.delete_draft')}
          </ChromeButton>
        )}
      </div>
    </FormSection>
  );
}

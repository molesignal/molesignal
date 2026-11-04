import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Bot, Copy, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as incidentsApi from '@/api/incidents';
import * as postmortemsApi from '@/api/postmortems';
import type { ActionItem, Postmortem, PostmortemStatus } from '@/api/postmortems';
import * as ticketsApi from '@/api/tickets';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton, uiLabelClass } from '@/shell/chrome';
import { FormDrawer, FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import type { Incident } from '@/types/alerting';

type Translate = ReturnType<typeof useTranslation<'alerts'>>['t'];

/** A postmortem's Markdown to start from: the template's sections, filled with what the incident already says. */
export function postmortemTemplate(
  t: Translate,
  incident: Incident,
  entries: Array<{ at: number; kind: string }>,
  rootCause: string | null,
): string {
  const end = incident.resolvedAt ?? null;
  const minutes = end ? Math.max(1, Math.round((end - incident.createdAt) / 60_000_000)) : null;
  const lines = [
    `# ${incident.summary}`,
    '',
    `## ${t('postmortems.template.summary')}`,
    '',
    `## ${t('postmortems.template.impact')}`,
    `- ${t('postmortems.template.duration', { value: minutes ? t('postmortems.template.minutes', { count: minutes }) : t('postmortems.template.ongoing') })}`,
    `- ${t('postmortems.template.services', { value: incident.affectedServices.join(', ') || '—' })}`,
    `- ${t('postmortems.template.severity', { value: incident.severity })}`,
    '',
    `## ${t('postmortems.template.timeline')}`,
    `- ${formatMicrosActive(incident.createdAt, false)} ${t('postmortems.template.triggered')}`,
    ...entries.map((entry) => `- ${formatMicrosActive(entry.at, false)} ${t(`postmortems.template.kinds.${entry.kind}`, { defaultValue: entry.kind })}`),
    ...(end ? [`- ${formatMicrosActive(end, false)} ${t('postmortems.template.resolved')}`] : []),
    '',
    `## ${t('postmortems.template.root_cause')}`,
    rootCause ?? '',
    '',
    `## ${t('postmortems.template.went_well')}`,
    '',
    `## ${t('postmortems.template.went_badly')}`,
    '',
    `## ${t('postmortems.template.actions')}`,
    '',
  ];

  return lines.join('\n');
}

/** The text under the summary heading: what goes to a status page's public postmortem. */
export function summaryOf(body: string): string {
  const parts = body.split(/^## /m);
  const summary = parts[1] ?? '';

  return summary.split('\n').slice(1).join('\n').trim();
}

/** Opening an incident's postmortem: started from the template when it has none yet. */
export function useStartPostmortem(incident: Incident) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async () => {
      const [timeline, rca] = await Promise.all([
        incidentsApi.timeline(incident.id).catch(() => null),
        incidentsApi.getRca(incident.id).catch(() => null),
      ]);
      const body = postmortemTemplate(t, incident, timeline?.entries ?? [], rca?.summary ?? null);
      return postmortemsApi.create(incident.id, { title: incident.summary, body });
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['incident-postmortem', incident.id] });
      void queryClient.invalidateQueries({ queryKey: ['postmortems'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
}

/** Writing a postmortem: its text, status and owner, and the action items that come out of it. */
export function PostmortemDrawer({
  open,
  incidentId,
  incidentSummary,
  onClose,
}: {
  open: boolean;
  incidentId: string;
  incidentSummary: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const { nameOf } = useUsers();
  const access = useActionAccess({ permission: 'postmortems.manage' });
  const agentAccess = useActionAccess({ permission: 'agent.use' });
  const key = ['incident-postmortem', incidentId];
  const detail = useQuery({ queryKey: key, queryFn: () => postmortemsApi.forIncident(incidentId), enabled: open });
  const connectors = useQuery({ queryKey: ['ticket-connectors'], queryFn: ticketsApi.listConnectors, enabled: open, retry: false });
  const postmortem = detail.data?.postmortem ?? null;
  const actions = detail.data?.actions ?? [];
  const [title, setTitle] = React.useState('');
  const [body, setBody] = React.useState('');
  const [status, setStatus] = React.useState<PostmortemStatus>('draft');
  const [newAction, setNewAction] = React.useState('');

  React.useEffect(() => {
    if (!postmortem) return;
    setTitle(postmortem.title);
    setBody(postmortem.body);
    setStatus(postmortem.status);
  }, [postmortem]);

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: key });
    void queryClient.invalidateQueries({ queryKey: ['postmortems'] });
  };
  const fail = (error: unknown) => toast.error(toApiError(error).message);
  const save = useMutation({
    mutationFn: (current: Postmortem) => postmortemsApi.update(current.id, { title, body, status }),
    onSuccess: () => {
      toast.success(t('postmortems.saved'));
      refresh();
    },
    onError: fail,
  });
  const add = useMutation({
    mutationFn: (current: Postmortem) => postmortemsApi.addAction(current.id, { title: newAction.trim() }),
    onSuccess: () => {
      setNewAction('');
      refresh();
    },
    onError: fail,
  });
  const toggle = useMutation({
    mutationFn: (item: ActionItem) => postmortemsApi.updateAction(item.postmortemId, item.id, { status: item.status === 'done' ? 'open' : 'done' }),
    onSuccess: refresh,
    onError: fail,
  });
  const remove = useMutation({
    mutationFn: (item: ActionItem) => postmortemsApi.deleteAction(item.postmortemId, item.id),
    onSuccess: refresh,
    onError: fail,
  });
  const ticket = useMutation({
    mutationFn: ({ item, connectorId }: { item: ActionItem; connectorId: string }) =>
      postmortemsApi.actionTicket(item.postmortemId, item.id, connectorId),
    onSuccess: (item) => {
      toast.success(t('postmortems.ticket_created', { key: item.ticketKey }));
      refresh();
    },
    onError: fail,
  });
  const connector = (connectors.data ?? []).find((candidate) => candidate.enabled);

  const draftWithAgent = () => {
    useMoleAgentStore.getState().start({
      prompt: t('postmortems.agent_prompt'),
      teamId: null,
      subject: { kind: 'incident', id: incidentId },
      references: [{ kind: 'incident', id: incidentId, label: incidentSummary }],
    });
    onClose();
  };

  const copySummary = async () => {
    try {
      await writeClipboardText(summaryOf(body));
      toast.success(t('postmortems.summary_copied'));
    } catch (error) {
      fail(error);
    }
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('postmortems.drawer_title')}
      subtitle={incidentSummary}
      width="64vw"
      footer={
        <>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.close')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!postmortem || save.isPending || access.disabled}
            disabledReason={access.reason}
            onClick={() => postmortem && save.mutate(postmortem)}
          >
            {tc('actions.save')}
          </ChromeButton>
        </>
      }
    >
      {!postmortem ? (
        <p className="font-sans text-sm text-tx-2">{detail.isLoading ? t('postmortems.loading') : t('postmortems.none')}</p>
      ) : (
        <div data-postmortem-drawer className="flex min-w-0 flex-col gap-4 font-sans">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-[minmax(0,1fr)_180px]">
            <FormField label={t('postmortems.title')}>
              <FormInput value={title} onChange={(event) => setTitle(event.target.value)} />
            </FormField>
            <FormField label={t('postmortems.status_label')}>
              <FormSelect
                value={status}
                onChange={(value) => setStatus(value as PostmortemStatus)}
                options={(['draft', 'in_review', 'published'] as const).map((value) => ({
                  value,
                  label: t(`postmortems.status.${value}`),
                }))}
              />
            </FormField>
          </div>
          <div className="flex flex-wrap gap-2">
            {agentAccess.allowed && (
              <ChromeButton size="sm" onClick={draftWithAgent}>
                <Bot className="h-3 w-3" />
                {t('postmortems.draft_with_agent')}
              </ChromeButton>
            )}
            <ChromeButton size="sm" onClick={() => void copySummary()}>
              <Copy className="h-3 w-3" />
              {t('postmortems.copy_summary')}
            </ChromeButton>
          </div>
          <FormField label={t('postmortems.body')}>
            <FormTextarea value={body} rows={18} onChange={(event) => setBody(event.target.value)} className="font-mono" />
          </FormField>
          <div>
            <div className={uiLabelClass}>{t('postmortems.actions_title')}</div>
            <ul data-postmortem-actions className="mt-2 flex flex-col gap-1.5">
              {actions.map((item) => (
                <li key={item.id} data-postmortem-action={item.id} className="flex min-w-0 items-center gap-2 text-sm">
                  <Checkbox checked={item.status === 'done'} aria-label={item.title} disabled={access.disabled} onCheckedChange={() => toggle.mutate(item)} />
                  <span className={cn('min-w-0 flex-1 truncate', item.status === 'done' ? 'text-tx-3 line-through' : 'text-tx-0')}>
                    {item.title}
                  </span>
                  {item.ownerId && <span className="shrink-0 text-xs text-tx-2">{nameOf(item.ownerId)}</span>}
                  {item.ticketKey ? (
                    <a href={item.ticketUrl} target="_blank" rel="noreferrer" className="shrink-0 text-xs text-indigo-soft underline-offset-2 hover:underline">
                      {item.ticketKey}
                    </a>
                  ) : (
                    connector && (
                      <ChromeButton
                        size="sm"
                        variant="ghost"
                        disabled={ticket.isPending || access.disabled}
                        onClick={() => ticket.mutate({ item, connectorId: connector.id })}
                      >
                        {t('postmortems.make_ticket')}
                      </ChromeButton>
                    )
                  )}
                  <IconButton
                    aria-label={t('postmortems.remove_action', { title: item.title })}
                    disabled={access.disabled}
                    className="enabled:hover:text-red-soft"
                    onClick={() => remove.mutate(item)}
                  >
                    <Trash2 className="h-3 w-3" />
                  </IconButton>
                </li>
              ))}
            </ul>
            <form
              className="mt-2 flex items-center gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (newAction.trim()) add.mutate(postmortem);
              }}
            >
              <FormInput
                value={newAction}
                onChange={(event) => setNewAction(event.target.value)}
                placeholder={t('postmortems.action_placeholder')}
                aria-label={t('postmortems.action_placeholder')}
                disabled={access.disabled}
              />
              <ChromeButton size="sm" type="submit" disabled={access.disabled || add.isPending || !newAction.trim()}>
                <Plus className="h-3 w-3" />
                {t('postmortems.add_action')}
              </ChromeButton>
            </form>
          </div>
        </div>
      )}
    </FormDrawer>
  );
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Megaphone, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as responseApi from '@/api/response';
import { RESPONSE_ROLES, type IncidentTask, type ResponseRole } from '@/api/response';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { Incident } from '@/types/alerting';

import { IncidentSection } from './Section';

/** A major incident's response: declaring it, the three roles, the war room and meeting, tasks and updates. */
export function IncidentMajor({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const { users, nameOf } = useUsers();
  const access = useActionAccess({ permission: 'alerts.incidents.escalate' });
  const [updating, setUpdating] = React.useState(false);
  const [title, setTitle] = React.useState('');
  const key = ['incident-response', incident.id];
  const query = useQuery({ queryKey: key, queryFn: () => responseApi.get(incident.id), retry: false });
  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: key });
    void queryClient.invalidateQueries({ queryKey: ['incident-timeline', incident.id] });
  };
  const fail = (error: unknown) => toast.error(toApiError(error).message);
  const declare = useMutation({
    mutationFn: () => responseApi.declare(incident.id),
    onSuccess: () => {
      toast.success(t('major.declared'));
      refresh();
    },
    onError: fail,
  });
  const role = useMutation({
    mutationFn: ({ name, userId }: { name: ResponseRole; userId: string | null }) => responseApi.assignRole(incident.id, name, userId),
    onSuccess: refresh,
    onError: fail,
  });
  const addTask = useMutation({
    mutationFn: (text: string) => responseApi.addTask(incident.id, text),
    onSuccess: () => {
      setTitle('');
      refresh();
    },
    onError: fail,
  });
  const toggleTask = useMutation({
    mutationFn: (task: IncidentTask) => responseApi.updateTask(incident.id, task.id, { done: !task.done }),
    onSuccess: refresh,
    onError: fail,
  });
  const removeTask = useMutation({
    mutationFn: (task: IncidentTask) => responseApi.deleteTask(incident.id, task.id),
    onSuccess: refresh,
    onError: fail,
  });
  const response = query.data?.response ?? null;
  const tasks = query.data?.tasks ?? [];
  const closed = incident.status === 'resolved' || incident.status === 'closed';

  if (!response && closed) return null;

  return (
    <IncidentSection title={t('drawer.incident_sections.major')} icon={<Megaphone className="h-3.5 w-3.5" />} surface={surface}>
      <div data-incident-major className="flex flex-col gap-3 font-sans text-xs">
        {!response ? (
          <div className="flex items-center gap-3">
            <p className="min-w-0 flex-1 text-tx-2">{t('major.hint')}</p>
            <ChromeButton
              size="sm"
              variant="primary"
              disabled={declare.isPending || access.disabled}
              disabledReason={access.reason}
              onClick={() => declare.mutate()}
            >
              {t('major.declare')}
            </ChromeButton>
          </div>
        ) : (
          <>
            <p className="text-tx-2">
              {response.declaredBy
                ? t('major.declared_by', { name: nameOf(response.declaredBy) })
                : t('major.declared_by_rule')}
            </p>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              {RESPONSE_ROLES.map((name) => (
                <FormField key={name} label={t(`major.roles.${name}`)}>
                  <FormSelect
                    value={response.roles[name] ?? ''}
                    onChange={(userId) => role.mutate({ name, userId: userId || null })}
                    ariaLabel={t(`major.roles.${name}`)}
                    disabled={access.disabled}
                    options={[
                      { value: '', label: t('major.unassigned') },
                      ...users.map((user) => ({ value: user.id, label: user.name })),
                    ]}
                  />
                </FormField>
              ))}
            </div>
            {(response.warRoom || response.meetingUrl) && (
              <div className="flex flex-wrap gap-x-4 gap-y-1">
                {response.warRoom &&
                  (response.warRoom.url ? (
                    <a
                      href={response.warRoom.url}
                      target="_blank"
                      rel="noreferrer"
                      className="text-indigo-soft underline-offset-2 hover:underline"
                    >
                      {t('major.war_room', { platform: t(`major.platforms.${response.warRoom.platform}`, { defaultValue: response.warRoom.platform }) })}
                    </a>
                  ) : (
                    // DingTalk and WeCom groups have no join link: the members find them in their client.
                    <span data-war-room-made className="text-tx-1">
                      {t('major.war_room_made', {
                        platform: t(`major.platforms.${response.warRoom.platform}`, { defaultValue: response.warRoom.platform }),
                      })}
                    </span>
                  ))}
                {response.meetingUrl && (
                  <a href={response.meetingUrl} target="_blank" rel="noreferrer" className="text-indigo-soft underline-offset-2 hover:underline">
                    {t('major.meeting')}
                  </a>
                )}
              </div>
            )}
            <div>
              <div className="mb-1 font-strong text-tx-1">{t('major.tasks')}</div>
              {tasks.length === 0 && <p className="text-tx-3">{t('major.no_tasks')}</p>}
              <ul data-incident-tasks className="flex flex-col gap-1">
                {tasks.map((task) => (
                  <li key={task.id} data-incident-task={task.id} className="flex min-w-0 items-center gap-2">
                    <Checkbox
                      checked={task.done}
                      disabled={access.disabled}
                      aria-label={task.title}
                      onCheckedChange={() => toggleTask.mutate(task)}
                    />
                    <span className={cn('min-w-0 flex-1 truncate text-sm', task.done ? 'text-tx-3 line-through' : 'text-tx-0')}>
                      {task.title}
                    </span>
                    {task.done && task.doneBy && <span className="shrink-0 text-tx-3">{nameOf(task.doneBy)}</span>}
                    <IconButton
                      aria-label={t('major.remove_task', { title: task.title })}
                      disabled={access.disabled}
                      disabledReason={access.reason}
                      className="enabled:hover:text-red-soft"
                      onClick={() => removeTask.mutate(task)}
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
                  if (title.trim()) addTask.mutate(title.trim());
                }}
              >
                <FormInput
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder={t('major.task_placeholder')}
                  aria-label={t('major.task_placeholder')}
                  disabled={access.disabled}
                />
                <ChromeButton size="sm" type="submit" disabled={access.disabled || addTask.isPending || !title.trim()}>
                  <Plus className="h-3 w-3" />
                  {t('major.add_task')}
                </ChromeButton>
              </form>
            </div>
            <div>
              <ChromeButton size="sm" disabled={access.disabled} disabledReason={access.reason} onClick={() => setUpdating(true)}>
                {t('major.post_update')}
              </ChromeButton>
            </div>
          </>
        )}
      </div>
      {updating && <StatusUpdateDialog incident={incident} onClose={() => setUpdating(false)} onSent={refresh} />}
    </IncidentSection>
  );
}

function StatusUpdateDialog({ incident, onClose, onSent }: { incident: Incident; onClose: () => void; onSent: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const [update, setUpdate] = React.useState({ impact: '', current: '', next: '' });
  const send = useMutation({
    mutationFn: () => responseApi.statusUpdate(incident.id, update),
    onSuccess: () => {
      toast.success(t('major.update_sent'));
      onSent();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('major.post_update')}</DialogTitle>
          <DialogDescription>{t('major.update_hint')}</DialogDescription>
        </DialogHeader>
        <form
          id="status-update-form"
          data-status-update
          className="flex min-w-0 flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            send.mutate();
          }}
        >
          {(['impact', 'current', 'next'] as const).map((part) => (
            <FormField key={part} label={t(`major.update.${part}`)}>
              <FormTextarea rows={2} value={update[part]} onChange={(event) => setUpdate((current) => ({ ...current, [part]: event.target.value }))} />
            </FormField>
          ))}
        </form>
        <DialogFooter>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          <ChromeButton type="submit" form="status-update-form" variant="primary" disabled={send.isPending}>
            {t('major.send_update')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

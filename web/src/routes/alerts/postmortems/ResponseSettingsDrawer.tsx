import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import * as responseApi from '@/api/response';
import { MEETING_MODES, MEETING_SECRETS, type MeetingMode, type ResponseSettings } from '@/api/response';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { FormDrawer, FormField, FormInput, FormSection, FormSelect, FormSubmitFooter, FormTextarea } from '@/shell/FormDrawer';
import { prioritiesOf, useImportance } from '@/shell/incident/Importance';
import { JsonEditor } from '@/shell/JsonEditor';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import type { Priority, Severity } from '@/types/alerting';

const SEVERITIES: Severity[] = ['critical', 'error', 'warning', 'info'];

/** The settings with every meeting's fields there, so switching meetings has something to fill. */
function withDefaults(settings: ResponseSettings): ResponseSettings {
  // Older or partial answers may leave a meeting out: read them as possibly missing.
  const given = settings as Partial<ResponseSettings>;

  return {
    ...settings,
    meetingConnectorId: given.meetingConnectorId ?? null,
    tencentMeeting: { appId: '', sdkId: '', secretId: '', userid: '', ...given.tencentMeeting },
    zoom: { accountId: '', clientId: '', host: '', ...given.zoom },
    googleMeet: { organizer: '', ...given.googleMeet },
    secretsSet: given.secretsSet ?? [],
    warRoomTypes: given.warRoomTypes ?? [],
  };
}

/** How major incidents are run: the war room's connector, the meeting, the checklist, who hears updates, and which incidents are major from the start. */
export function ResponseSettingsDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const levels = useImportance();
  const priorities = prioritiesOf(levels);
  const queryClient = useQueryClient();
  const { users } = useUsers();
  const manageAccess = useActionAccess({ permission: 'alerts.response.manage' });
  const current = useQuery({ queryKey: ['response-settings'], queryFn: responseApi.settings, enabled: open });
  const connectors = useQuery({ queryKey: ['notify', 'connectors'], queryFn: () => notifyApi.listConnectors(), enabled: open, retry: false });
  const [draft, setDraft] = React.useState<ResponseSettings | null>(null);
  const [checklist, setChecklist] = React.useState('');
  const [services, setServices] = React.useState('');
  const [secrets, setSecrets] = React.useState<Record<string, string>>({});

  React.useEffect(() => {
    if (!open || !current.data) return;
    setDraft(withDefaults(current.data));
    setChecklist(current.data.checklist.join('\n'));
    setServices(current.data.auto.conditions.find((condition) => condition.key === 'service')?.values.join(', ') ?? '');
    setSecrets({});
  }, [current.data, open]);

  const save = useMutation({
    mutationFn: (settings: ResponseSettings) => {
      const names = services
        .split(',')
        .map((name) => name.trim())
        .filter(Boolean);
      return responseApi.saveSettings(
        {
          ...settings,
          checklist: checklist
            .split('\n')
            .map((line) => line.trim())
            .filter(Boolean),
          auto: { ...settings.auto, conditions: names.length > 0 ? [{ key: 'service', operator: 'in', values: names }] : [] },
        },
        secrets,
      );
    },
    onSuccess: () => {
      toast.success(t('major.settings.saved'));
      void queryClient.invalidateQueries({ queryKey: ['response-settings'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const set = (patch: Partial<ResponseSettings>) => setDraft((value) => (value ? { ...value, ...patch } : value));
  const groupTypes = draft?.warRoomTypes ?? [];
  const groupApps = (connectors.data ?? []).filter((connector) => groupTypes.includes(connector.connectorType));
  const feishuApps = (connectors.data ?? []).filter((connector) => connector.connectorType === 'lark_app');
  const roomType = groupApps.find((connector) => connector.id === draft?.warRoomConnectorId)?.connectorType;
  const platform = (type: string) => t(`major.platforms.${type}`, { defaultValue: type });

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('major.settings.title')}
      subtitle={t('major.settings.subtitle')}
      footer={
        <FormSubmitFooter
          formId="response-settings-form"
          busy={save.isPending}
          disabled={manageAccess.disabled || !draft}
          disabledReason={manageAccess.reason}
          onCancel={onClose}
        />
      }
    >
      {draft && (
        <form
          id="response-settings-form"
          data-response-settings
          onSubmit={(event) => {
            event.preventDefault();
            save.mutate(draft);
          }}
        >
          <FormSection title={t('major.settings.war_room')} description={t('major.settings.war_room_hint')}>
            <FormField
              label={t('major.settings.connector')}
              {...(roomType && t(`major.settings.room_notes.${roomType}`, { defaultValue: '' })
                ? { hint: t(`major.settings.room_notes.${roomType}`) }
                : {})}
            >
              <FormSelect
                value={draft.warRoomConnectorId ?? ''}
                onChange={(value) => set({ warRoomConnectorId: value || null })}
                options={[
                  { value: '', label: t('major.settings.no_war_room') },
                  ...groupApps.map((connector) => ({ value: connector.id, label: `${connector.name} · ${platform(connector.connectorType)}` })),
                ]}
              />
            </FormField>
            <FormField label={t('major.settings.meeting')}>
              <FormSelect
                value={draft.meeting}
                onChange={(value) => set({ meeting: value as MeetingMode })}
                options={MEETING_MODES.map((mode) => ({ value: mode, label: t(`major.settings.meetings.${mode}`) }))}
              />
            </FormField>
            <MeetingFields
              draft={draft}
              set={set}
              feishuApps={feishuApps}
              secrets={secrets}
              setSecret={(key, value) => setSecrets((current) => ({ ...current, [key]: value }))}
            />
          </FormSection>
          <FormSection title={t('major.settings.checklist')} description={t('major.settings.checklist_hint')}>
            <FormTextarea value={checklist} rows={5} onChange={(event) => setChecklist(event.target.value)} />
          </FormSection>
          <FormSection title={t('major.settings.stakeholders')} description={t('major.settings.stakeholders_hint')}>
            <div className="flex flex-wrap gap-2">
              {users.map((user) => (
                <label key={user.id} className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
                  <Checkbox
                    checked={draft.stakeholderIds.includes(user.id)}
                    onCheckedChange={(next) =>
                      set({
                        stakeholderIds:
                          next === true ? [...draft.stakeholderIds, user.id] : draft.stakeholderIds.filter((id) => id !== user.id),
                      })
                    }
                  />
                  <span>{user.name}</span>
                </label>
              ))}
            </div>
          </FormSection>
          <FormSection title={t('major.settings.auto')} description={t('major.settings.auto_hint')}>
            <label className="flex min-h-9 items-center gap-2 rounded-md bg-[var(--control-surface)] px-2.5 py-2 font-sans text-sm text-tx-0">
              <Checkbox checked={draft.auto.enabled} onCheckedChange={(next) => set({ auto: { ...draft.auto, enabled: next === true } })} />
              <span>{t('major.settings.auto_enabled')}</span>
            </label>
            {draft.auto.enabled && (
              <div className="grid grid-cols-2 gap-2">
                <FormField label={t('tickets.drawer.auto_severity')}>
                  <FormSelect
                    value={draft.auto.minSeverity ?? ''}
                    onChange={(value) => set({ auto: { ...draft.auto, minSeverity: (value || null) as Severity | null } })}
                    options={[
                      { value: '', label: t('routing.drawer.any_severity') },
                      ...SEVERITIES.map((severity) => ({ value: severity, label: t(`history.severity.${severity}`) })),
                    ]}
                  />
                </FormField>
                <FormField label={t('major.settings.auto_priority')} hint={t('major.settings.auto_priority_hint')}>
                  <FormSelect
                    value={draft.auto.minPriority ?? ''}
                    onChange={(value) =>
                      set({ auto: { ...draft.auto, minPriority: (value || null) as Priority | null } })
                    }
                    options={[
                      { value: '', label: t('major.settings.any_priority') },
                      ...priorities.map((priority) => ({
                        value: priority,
                        label: t('major.settings.at_least_priority', { priority }),
                      })),
                    ]}
                  />
                </FormField>
                <FormField label={t('tickets.drawer.auto_services')}>
                  <FormInput value={services} onChange={(event) => setServices(event.target.value)} placeholder="checkout, payments" />
                </FormField>
              </div>
            )}
          </FormSection>
        </form>
      )}
    </FormDrawer>
  );
}

/** What the chosen meeting needs: a Feishu App, a fixed link, or an account and its secret. A saved secret is never shown again. */
function MeetingFields({
  draft,
  set,
  feishuApps,
  secrets,
  setSecret,
}: {
  draft: ResponseSettings;
  set: (patch: Partial<ResponseSettings>) => void;
  feishuApps: Array<{ id: string; name: string }>;
  secrets: Record<string, string>;
  setSecret: (key: string, value: string) => void;
}) {
  const { t } = useTranslation('alerts');
  const secretKey = MEETING_SECRETS[draft.meeting];
  const kept = secretKey ? draft.secretsSet.includes(secretKey) : false;
  const keptHint = kept ? t('major.settings.secret_kept') : '';
  const hint = t(`major.settings.meeting_hints.${draft.meeting}`, { defaultValue: '' });
  const secretInput = (label: string) =>
    secretKey && (
      <FormField label={label} required={!kept} {...(keptHint ? { hint: keptHint } : {})}>
        <FormInput
          type="password"
          autoComplete="new-password"
          value={secrets[secretKey] ?? ''}
          onChange={(event) => setSecret(secretKey, event.target.value)}
        />
      </FormField>
    );

  return (
    <div data-meeting-fields={draft.meeting} className="flex min-w-0 flex-col gap-3">
      {hint && <p className="font-sans text-xs leading-relaxed text-tx-3">{hint}</p>}
      {draft.meeting === 'fixed_url' && (
        <FormField label={t('major.settings.meeting_url')}>
          <FormInput value={draft.meetingUrl} onChange={(event) => set({ meetingUrl: event.target.value })} placeholder="https://meet.example.com/war-room" />
        </FormField>
      )}
      {draft.meeting === 'feishu_vc' && (
        <FormField label={t('major.settings.meeting_connector')}>
          <FormSelect
            value={draft.meetingConnectorId ?? ''}
            onChange={(value) => set({ meetingConnectorId: value || null })}
            options={[
              { value: '', label: t('major.settings.same_as_war_room') },
              ...feishuApps.map((connector) => ({ value: connector.id, label: connector.name })),
            ]}
          />
        </FormField>
      )}
      {draft.meeting === 'tencent_meeting' && (
        <div className="grid grid-cols-2 gap-2">
          <FormField label="AppId" required>
            <FormInput value={draft.tencentMeeting.appId} onChange={(event) => set({ tencentMeeting: { ...draft.tencentMeeting, appId: event.target.value } })} />
          </FormField>
          <FormField label="SdkId" hint={t('major.settings.tencent.sdk_id_hint')}>
            <FormInput value={draft.tencentMeeting.sdkId} onChange={(event) => set({ tencentMeeting: { ...draft.tencentMeeting, sdkId: event.target.value } })} />
          </FormField>
          <FormField label="SecretId" required>
            <FormInput value={draft.tencentMeeting.secretId} onChange={(event) => set({ tencentMeeting: { ...draft.tencentMeeting, secretId: event.target.value } })} />
          </FormField>
          {secretInput('SecretKey')}
          <FormField label={t('major.settings.tencent.userid')} hint={t('major.settings.tencent.userid_hint')} required>
            <FormInput value={draft.tencentMeeting.userid} onChange={(event) => set({ tencentMeeting: { ...draft.tencentMeeting, userid: event.target.value } })} />
          </FormField>
        </div>
      )}
      {draft.meeting === 'zoom' && (
        <div className="grid grid-cols-2 gap-2">
          <FormField label="Account ID" required>
            <FormInput value={draft.zoom.accountId} onChange={(event) => set({ zoom: { ...draft.zoom, accountId: event.target.value } })} />
          </FormField>
          <FormField label="Client ID" required>
            <FormInput value={draft.zoom.clientId} onChange={(event) => set({ zoom: { ...draft.zoom, clientId: event.target.value } })} />
          </FormField>
          {secretInput('Client Secret')}
          <FormField label={t('major.settings.zoom.host')} hint={t('major.settings.zoom.host_hint')} required>
            <FormInput value={draft.zoom.host} onChange={(event) => set({ zoom: { ...draft.zoom, host: event.target.value } })} />
          </FormField>
        </div>
      )}
      {draft.meeting === 'google_meet' && secretKey && (
        <>
          <FormField label={t('major.settings.google.organizer')} hint={t('major.settings.google.organizer_hint')} required>
            <FormInput
              type="email"
              value={draft.googleMeet.organizer}
              onChange={(event) => set({ googleMeet: { ...draft.googleMeet, organizer: event.target.value } })}
            />
          </FormField>
          <FormField label={t('major.settings.google.credentials')} required={!kept} {...(keptHint ? { hint: keptHint } : {})}>
            <JsonEditor
              value={secrets[secretKey] ?? ''}
              onChange={(value) => setSecret(secretKey, value)}
              ariaLabel={t('major.settings.google.credentials')}
              minHeight={112}
              placeholder='{"type": "service_account", "client_email": "…", "private_key": "…"}'
            />
          </FormField>
        </>
      )}
    </div>
  );
}

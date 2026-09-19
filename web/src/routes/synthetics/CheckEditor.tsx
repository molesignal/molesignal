import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as escalationsApi from '@/api/escalations';
import * as syntheticsApi from '@/api/synthetics';
import type { MonitorDetail, MonitorKind, ProbeLocation } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  AssertionEditor,
  CheckToggle,
  intervalLabel,
  KIND_OPTIONS,
  supportsAssertions,
  TargetFields,
  targetIsValid,
} from './editor/fields';
import {
  applyPreset,
  draftFromDetail,
  emptyDraft,
  inputFromDraft,
  type CheckDraft,
} from './editor/model';
import { saveCheckDraft, type CheckEditorMode } from './editor/save';

export function CheckEditor({
  open,
  onOpenChange,
  mode,
  detail,
  locations,
  onSaved,
  preset,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: CheckEditorMode;
  detail: MonitorDetail | undefined;
  locations: ProbeLocation[];
  onSaved: (monitorId: string) => void;
  preset: string | null | undefined;
}) {
  const { t } = useTranslation('synthetics');
  const queryClient = useQueryClient();
  const escalationQuery = useQuery({
    queryKey: ['escalation-policies'],
    queryFn: escalationsApi.list,
    enabled: open,
    staleTime: 30_000,
  });
  const [secrets, setSecrets] = React.useState<Awaited<ReturnType<typeof syntheticsApi.listSecrets>>>([]);
  const [draft, setDraft] = React.useState<CheckDraft>(() => emptyDraft(locations));

  React.useEffect(() => {
    if (!open) return;
    setDraft(
      detail && mode !== 'create'
        ? draftFromDetail(detail, locations, mode === 'clone')
        : applyPreset(emptyDraft(locations), preset ?? null),
    );
  }, [detail, locations, mode, open, preset]);

  React.useEffect(() => {
    if (!open) return;
    void syntheticsApi.listSecrets().then(setSecrets).catch(() => setSecrets([]));
  }, [open]);

  const save = useMutation({
    mutationFn: async () => {
      const input = inputFromDraft(draft, secrets);
      return saveCheckDraft(syntheticsApi, mode, detail, input);
    },
    onSuccess: async (monitorId) => {
      await queryClient.invalidateQueries({ queryKey: ['synthetics'] });
      toast.success(t(mode === 'clone' ? 'checks.cloned' : 'checks.saved'));
      onOpenChange(false);
      onSaved(monitorId);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const invalid =
    !draft.name.trim() ||
    !targetIsValid(draft) ||
    (draft.kind !== 'heartbeat' && draft.locationIds.length === 0);
  const patch = <K extends keyof CheckDraft>(key: K, value: CheckDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));
  const changeKind = (kind: MonitorKind) => {
    setDraft((current) => ({
      ...current,
      kind,
      intervalSeconds:
        kind === 'browser' && Number(current.intervalSeconds) < 60
          ? '60'
          : current.intervalSeconds,
      port:
        kind === 'ssh'
          ? '22'
          : kind === 'tls' || kind === 'tcp'
            ? current.port || '443'
            : current.port,
    }));
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t(`editor.${mode}_title`)}
      subtitle={t('editor.subtitle')}
      width={820}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          invalid={invalid}
          onCancel={() => onOpenChange(false)}
          submitLabel={mode === 'edit' ? t('actions.save_draft') : t('actions.create_check')}
          formId="synthetics-check-form"
        />
      }
    >
      <form
        id="synthetics-check-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid) save.mutate();
        }}
      >
        <FormSection title={t('editor.identity')} description={t('editor.identity_hint')}>
          <FormField label={t('editor.name')} required>
            <FormInput
              value={draft.name}
              onChange={(event) => patch('name', event.target.value)}
              placeholder={t('editor.name_placeholder')}
              disabled={mode === 'edit'}
            />
          </FormField>
          <FormField label={t('editor.description')}>
            <FormTextarea
              value={draft.description}
              onChange={(event) => patch('description', event.target.value)}
              placeholder={t('editor.description_placeholder')}
            />
          </FormField>
          <FormField label={t('editor.type')} required>
            <FormSelect
              value={draft.kind}
              onChange={(value) => changeKind(value as MonitorKind)}
              options={KIND_OPTIONS.map((kind) => ({ value: kind, label: t(`kinds.${kind}`) }))}
              disabled={mode === 'edit'}
            />
          </FormField>
        </FormSection>

        <FormSection title={t('editor.request')}>
          <TargetFields draft={draft} patch={patch} />
        </FormSection>

        {supportsAssertions(draft.kind) && (
          <FormSection title={t('editor.assertions')} description={t('editor.assertions_hint')}>
            <AssertionEditor
              assertions={draft.assertions}
              onChange={(assertions) => patch('assertions', assertions)}
            />
          </FormSection>
        )}

        <FormSection title={t('editor.schedule')}>
          {draft.kind !== 'heartbeat' && (
            <FormField label={t('editor.schedule_kind')}>
              <FormSelect
                value={draft.scheduleKind}
                onChange={(value) => patch('scheduleKind', value as CheckDraft['scheduleKind'])}
                options={[
                  { value: 'interval', label: t('editor.interval') },
                  { value: 'cron', label: 'Cron' },
                ]}
              />
            </FormField>
          )}
          {draft.scheduleKind === 'cron' && draft.kind !== 'heartbeat' ? (
            <FormRow className="grid-cols-1 sm:grid-cols-2">
              <FormField label={t('editor.cron')} required>
                <FormInput value={draft.cron} onChange={(event) => patch('cron', event.target.value)} />
              </FormField>
              <FormField label={t('editor.timezone')} required>
                <FormInput value={draft.timezone} onChange={(event) => patch('timezone', event.target.value)} />
              </FormField>
            </FormRow>
          ) : (
            <FormField label={t('editor.interval')} required>
              <FormSelect
                value={draft.intervalSeconds}
                onChange={(value) => patch('intervalSeconds', value)}
                options={(draft.kind === 'browser' ? [60, 300, 600, 1800, 3600] : [30, 60, 300, 600, 1800, 3600]).map(
                  (seconds) => ({ value: String(seconds), label: intervalLabel(seconds) }),
                )}
              />
            </FormField>
          )}
          <FormRow className="grid-cols-1 sm:grid-cols-2">
            <FormField label={t('editor.timeout')}>
              <FormInput type="number" min={100} max={300000} value={draft.timeoutMillis} onChange={(event) => patch('timeoutMillis', event.target.value)} />
            </FormField>
            <FormField label={t('editor.retries')}>
              <FormSelect value={draft.retries} onChange={(value) => patch('retries', value)} options={['0', '1', '2']} />
            </FormField>
          </FormRow>
          <FormRow className="grid-cols-1 sm:grid-cols-2">
            <FormField label={t('editor.failure_threshold')}>
              <FormInput type="number" min={1} value={draft.failureThreshold} onChange={(event) => patch('failureThreshold', event.target.value)} />
            </FormField>
            <FormField label={t('editor.recovery_threshold')}>
              <FormInput type="number" min={1} value={draft.recoveryThreshold} onChange={(event) => patch('recoveryThreshold', event.target.value)} />
            </FormField>
          </FormRow>
        </FormSection>

        {draft.kind !== 'heartbeat' && (
          <FormSection title={t('editor.locations')} description={t('editor.locations_hint')}>
            <FormChecklist
              options={locations
                .filter((location) => location.lifecycle === 'active')
                .map((location) => ({
                  value: location.id,
                  label: location.name,
                  hint: `${location.code} · ${t(`locations.${location.scope}`)}`,
                }))}
              selected={draft.locationIds}
              onChange={(locationIds) => patch('locationIds', locationIds)}
            />
          </FormSection>
        )}

        <FormSection>
          <FormField label={t('editor.escalation_policy')}>
            <FormSelect
              value={draft.escalationPolicyId}
              onChange={(value) => patch('escalationPolicyId', value)}
              options={[
                { value: '', label: t('editor.no_escalation_policy') },
                ...(escalationQuery.data ?? []).map((policy) => ({
                  value: policy.id,
                  label: policy.name,
                })),
              ]}
            />
          </FormField>
          <FormField label={t('editor.tags')} hint={t('editor.tags_hint')}>
            <FormInput value={draft.tags} onChange={(event) => patch('tags', event.target.value)} placeholder="production, checkout" />
          </FormField>
          <CheckToggle
            checked={draft.alertOnDegraded}
            onChange={(checked) => patch('alertOnDegraded', checked)}
            label={t('editor.alert_on_degraded')}
          />
          <CheckToggle
            checked={draft.alertOnFlaky}
            onChange={(checked) => patch('alertOnFlaky', checked)}
            label={t('editor.alert_on_flaky')}
          />
        </FormSection>
      </form>
    </FormDrawer>
  );
}

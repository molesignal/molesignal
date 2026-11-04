import { useQuery } from '@tanstack/react-query';
import { Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as notifyApi from '@/api/notify';
import { ChromeButton, IconButton, uiLabelClass } from '@/shell/chrome';
import { FormChecklist, FormInput } from '@/shell/FormDrawer';
import { TimeConditionField } from '@/shell/weekly-ranges/WeeklyWindowsFields';

import { Segmented } from './Segmented';
import {
  ALERT_SEVERITIES,
  ALERT_SOURCES,
  NOTIFY_ALERT_SOURCES,
  type AlertConditions,
  type AlertLabel,
  type AlertSourceKind,
  type AlertStage,
  type PipelineGraphModel,
} from '../PipelineGraph/model';

/**
 * When an alert pipeline runs and which alert events it is for: before the
 * incident opens or before a notification goes out (then for which policies),
 * the sources and severities it takes (none ticked: every one), labels an event
 * must have, each with its value, and the weekly hours it must open in or out
 * of. An event that does not match goes past the pipeline untouched.
 */
export function AlertSourceFields({
  model,
  onChange,
}: {
  model: PipelineGraphModel;
  onChange: (next: PipelineGraphModel) => void;
}) {
  const { t } = useTranslation('pipelines');
  const conditions = model.alert;
  const set = (patch: Partial<AlertConditions>) => onChange({ ...model, alert: { ...conditions, ...patch } });
  const setLabel = (index: number, patch: Partial<AlertLabel>) =>
    set({ labels: conditions.labels.map((label, at) => (at === index ? { ...label, ...patch } : label)) });
  const notify = conditions.stage === 'notify';
  const sources: readonly AlertSourceKind[] = notify ? NOTIFY_ALERT_SOURCES : ALERT_SOURCES;

  return (
    <div className="grid max-w-[720px] gap-[14px]">
      <div className="grid gap-1.5" data-alert-stage>
        <span className={uiLabelClass}>{t('alert_mode.stage.label')}</span>
        <div className="max-w-[420px]">
          <Segmented<AlertStage>
            label={t('alert_mode.stage.label')}
            value={conditions.stage}
            onChange={(stage) => set({ stage })}
            options={[
              { value: 'open', label: t('alert_mode.stage.open') },
              { value: 'notify', label: t('alert_mode.stage.notify') },
            ]}
          />
        </div>
        <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">
          {t(notify ? 'alert_mode.stage.notify_hint' : 'alert_mode.stage.open_hint')}
        </p>
      </div>
      {notify && <PolicyFields selected={conditions.policies} onChange={(policies) => set({ policies })} />}
      <p className="m-0 font-sans text-sm leading-relaxed text-tx-2">{t('alert_mode.source_hint')}</p>
      <div className="grid gap-x-[24px] gap-y-[12px] sm:grid-cols-2">
        <div className="grid content-start gap-1.5" role="group" aria-label={t('alert_mode.sources')}>
          <span className={uiLabelClass}>{t('alert_mode.sources')}</span>
          <FormChecklist
            options={sources.map((source) => ({ value: source, label: t(`alert_mode.source.${source}`) }))}
            selected={conditions.sources}
            onChange={(next) => set({ sources: next })}
          />
        </div>
        <div className="grid content-start gap-1.5" role="group" aria-label={t('alert_mode.severities')}>
          <span className={uiLabelClass}>{t('alert_mode.severities')}</span>
          <FormChecklist
            options={ALERT_SEVERITIES.map((severity) => ({ value: severity, label: t(`alert_mode.severity.${severity}`) }))}
            selected={conditions.severities}
            onChange={(severities) => set({ severities })}
          />
        </div>
      </div>

      <div className="grid gap-[6px]" role="group" aria-label={t('alert_mode.labels')}>
        <span className={uiLabelClass}>{t('alert_mode.labels')}</span>
        {conditions.labels.map((label, index) => (
          <div key={index} data-alert-label className="flex min-w-0 items-center gap-[8px]">
            <FormInput
              value={label.key}
              onChange={(event) => setLabel(index, { key: event.target.value })}
              placeholder="env"
              aria-label={t('alert_mode.label_key')}
              className="w-[180px] font-mono text-xs"
              autoComplete="off"
              spellCheck={false}
            />
            <span aria-hidden className="text-tx-3">
              =
            </span>
            <FormInput
              value={label.value}
              onChange={(event) => setLabel(index, { value: event.target.value })}
              placeholder="prod"
              aria-label={t('alert_mode.label_value')}
              className="min-w-0 flex-1 font-mono text-xs"
              autoComplete="off"
              spellCheck={false}
            />
            <IconButton
              aria-label={t('alert_mode.label_remove')}
              onClick={() => set({ labels: conditions.labels.filter((_, at) => at !== index) })}
            >
              <X aria-hidden className="h-3.5 w-3.5" />
            </IconButton>
          </div>
        ))}
        <ChromeButton
          size="sm"
          variant="ghost"
          className="justify-self-start text-tx-2"
          onClick={() => set({ labels: [...conditions.labels, { key: '', value: '' }] })}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('alert_mode.label_add')}
        </ChromeButton>
      </div>
      <TimeConditionField
        value={conditions.time}
        onChange={(time) => set({ time })}
        hint={t('alert_mode.time_hint')}
      />
    </div>
  );
}

/**
 * The notification policies a pipeline before notifications is for; none ticked
 * is every policy of an alert event. A policy that no longer exists stays listed
 * by its id until someone unticks it.
 */
function PolicyFields({
  selected,
  onChange,
}: {
  selected: readonly string[];
  onChange: (policies: string[]) => void;
}) {
  const { t } = useTranslation('pipelines');
  const policies = useQuery({
    queryKey: ['notify', 'policies'],
    queryFn: notifyApi.listPolicies,
    staleTime: 60_000,
    retry: false,
  });
  const known = (policies.data ?? []).filter((policy) => policy.eventType.startsWith('alert.'));
  const options = [
    ...known.map((policy) => ({ value: policy.id, label: policy.name })),
    ...selected
      .filter((id) => !known.some((policy) => policy.id === id))
      .map((id) => ({ value: id, label: id })),
  ];

  return (
    <div className="grid content-start gap-1.5" role="group" aria-label={t('alert_mode.policies')} data-alert-policies>
      <span className={uiLabelClass}>{t('alert_mode.policies')}</span>
      {options.length > 0 ? (
        <FormChecklist options={options} selected={[...selected]} onChange={onChange} />
      ) : (
        <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">{t('alert_mode.policies_none')}</p>
      )}
      <p className="m-0 font-sans text-xs leading-relaxed text-tx-3">{t('alert_mode.policies_hint')}</p>
    </div>
  );
}

/**
 * Where an alert pipeline's events go: back into the incident that opens, or into
 * the one notification, as the steps left them.
 */
export function AlertDestinationNote({ stage }: { stage: AlertStage }) {
  const { t } = useTranslation('pipelines');

  return (
    <p className="m-0 max-w-[640px] font-sans text-sm leading-relaxed text-tx-2">
      {t(stage === 'notify' ? 'alert_mode.write_back_notify_hint' : 'alert_mode.write_back_hint')}
    </p>
  );
}

import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as nodeAgentsApi from '@/api/nodeAgents';
import type { NodeAgent } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormField, FormGroup, FormInput, FormRow, FormSection, FormSubmitFooter } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { nodeName } from '../model';
import {
  COLLECTOR_GROUPS,
  type ConfigDraft,
  type ConfigField,
  configFromDraft,
  draftFromConfig,
  type NodeConfigDocument,
} from './config';

const RANGES: Record<ConfigField, string> = {
  intervalSecs: '1–3600',
  frequencyHz: '1–1000',
  maxWorkloads: '1–1024',
  offcpuMinDurationUs: '0–10000000',
  dropwatchFilter: '',
  reclaimThresholdMs: '1–60000',
};

/** A checklist of collectors in columns, with one switch for the whole group. */
function CollectorChecklist({
  label,
  names,
  enabled,
  onChange,
}: {
  label: string;
  names: readonly string[];
  enabled: readonly string[];
  onChange: (enabled: string[]) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const on = names.filter((name) => enabled.includes(name)).length;
  const whole = on === names.length ? true : on === 0 ? false : 'indeterminate';
  const others = enabled.filter((name) => !names.includes(name));

  return (
    <FormGroup label={label}>
      <label className="flex items-center gap-2 text-xs text-tx-2">
        <Checkbox
          checked={whole}
          onCheckedChange={(checked) => onChange(checked === true ? [...others, ...names] : others)}
        />
        {t('node_agents.config.collectors_running', { on, total: names.length })}
      </label>
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 rounded-md bg-bg-2 p-3 sm:grid-cols-3">
        {names.map((name) => (
          <label key={name} className="flex min-w-0 items-center gap-2 font-code text-xs text-tx-1">
            <Checkbox
              checked={enabled.includes(name)}
              aria-label={name}
              onCheckedChange={(checked) =>
                onChange(checked === true ? [...enabled, name] : enabled.filter((item) => item !== name))
              }
            />
            <span className="truncate">{name}</span>
          </label>
        ))}
      </div>
    </FormGroup>
  );
}

/** What the node is told to run, as the next revision: collectors, continuous profiling and options. */
export function ConfigDrawer({
  node,
  onClose,
  onDone,
}: {
  node: NodeAgent | undefined;
  onClose: () => void;
  onDone: (node: NodeAgent) => void;
}) {
  const { t } = useTranslation('infrastructure');

  const initial = React.useMemo(
    () =>
      draftFromConfig(
        (node?.desiredConfig?.config ?? {}) as NodeConfigDocument,
        node?.collectorStates.map((collector) => collector.name) ?? [],
      ),
    [node],
  );

  const [draft, setDraft] = React.useState<ConfigDraft>(initial);
  React.useEffect(() => setDraft(initial), [initial]);
  const built = configFromDraft(draft);
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  const set = <K extends keyof ConfigDraft>(field: K, value: ConfigDraft[K]) => setDraft({ ...draft, [field]: value });

  const save = useMutation({
    mutationFn: (config: NodeConfigDocument) =>
      nodeAgentsApi.setNodeConfig(node?.id ?? '', config as Record<string, unknown>),
    onSuccess: (updated) => {
      toast.success(t('node_agents.config.saved', { revision: updated.desiredConfig?.revision ?? 0 }));
      onDone(updated);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const number = (field: ConfigField, label: string) => (
    <FormField label={label} hint={t('node_agents.config.range', { range: RANGES[field] })}>
      <FormInput
        inputMode="numeric"
        value={draft[field]}
        placeholder={t('node_agents.spec.node_default')}
        onChange={(event) => set(field, event.target.value)}
      />
      {'field' in built && built.field === field && (
        <span className="text-xs text-red">{t('node_agents.config.out_of_range', { range: RANGES[field] })}</span>
      )}
    </FormField>
  );

  const allCollectors = [...Object.values(COLLECTOR_GROUPS).flat(), ...draft.extra];

  return (
    <FormDrawer
      open={Boolean(node)}
      onOpenChange={(open) => !open && onClose()}
      title={t('node_agents.config.title', { name: node ? nodeName(node) : '' })}
      subtitle={t('node_agents.config.subtitle', { revision: (node?.desiredConfig?.revision ?? 0) + 1 })}
      dirty={dirty}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          invalid={'field' in built || !dirty}
          onCancel={onClose}
          formId="node-config"
          submitLabel={t('node_agents.config.save')}
        />
      }
    >
      <form
        id="node-config"
        onSubmit={(event) => {
          event.preventDefault();
          if ('config' in built) save.mutate(built.config);
        }}
      >
        <FormSection title={t('node_agents.config.collectors')} description={t('node_agents.config.collectors_hint')}>
          {(Object.keys(COLLECTOR_GROUPS) as (keyof typeof COLLECTOR_GROUPS)[]).map((group) => (
            <CollectorChecklist
              key={group}
              label={t(`node_agents.config.groups.${group}`)}
              names={COLLECTOR_GROUPS[group]}
              enabled={draft.enabled}
              onChange={(enabled) => set('enabled', enabled)}
            />
          ))}
          {draft.extra.length > 0 && (
            <CollectorChecklist
              label={t('node_agents.config.groups.reported')}
              names={draft.extra}
              enabled={draft.enabled}
              onChange={(enabled) => set('enabled', enabled)}
            />
          )}
          <div>
            <ChromeButton type="button" size="sm" onClick={() => set('enabled', allCollectors)}>
              {t('node_agents.config.enable_all')}
            </ChromeButton>
          </div>
        </FormSection>

        <FormSection title={t('node_agents.config.profiling')} description={t('node_agents.config.profiling_hint')}>
          <FormRow>
            <FormGroup label={t('node_agents.config.oncpu')}>
              <Switch
                checked={draft.oncpu}
                aria-label={t('node_agents.config.oncpu')}
                onCheckedChange={(checked) => set('oncpu', checked)}
              />
            </FormGroup>
            <FormGroup label={t('node_agents.config.offcpu')}>
              <Switch
                checked={draft.offcpu}
                aria-label={t('node_agents.config.offcpu')}
                onCheckedChange={(checked) => set('offcpu', checked)}
              />
            </FormGroup>
          </FormRow>
          <FormRow>
            {number('intervalSecs', t('node_agents.config.interval_secs'))}
            {number('frequencyHz', t('node_agents.spec.frequency'))}
          </FormRow>
          <FormRow>
            {number('maxWorkloads', t('node_agents.config.max_workloads'))}
            {number('offcpuMinDurationUs', t('node_agents.config.offcpu_min_duration_us'))}
          </FormRow>
        </FormSection>

        <FormSection title={t('node_agents.config.options')}>
          <FormField label={t('node_agents.config.dropwatch_filter')} hint={t('node_agents.config.dropwatch_filter_hint')}>
            <FormInput
              value={draft.dropwatchFilter}
              placeholder="tcp"
              className="font-code"
              onChange={(event) => set('dropwatchFilter', event.target.value)}
            />
          </FormField>
          {number('reclaimThresholdMs', t('node_agents.config.reclaim_threshold_ms'))}
        </FormSection>
      </form>
    </FormDrawer>
  );
}

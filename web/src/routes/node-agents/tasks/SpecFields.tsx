import { useTranslation } from 'react-i18next';

import type { NodeTaskKind } from '@/api/nodeAgents';
import { FormField, FormGroup, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { Switch } from '@/shell/ui/switch';

import {
  CONTAINER_RULES,
  defaultDuration,
  DEVICE_SCOPES,
  PROFILE_MODES,
  RULES,
  type SpecDraft,
  type SpecField,
  type SpecProblem,
  TARGETS,
  TOOLS,
  type ToolName,
} from './spec';

/**
 * The spec of a task as fields: for a tool, its window and the options that tool takes; for an
 * AutoTracing run, the scenario; for a profile, its window, target, frequency and mode.
 */
export function SpecFields({
  kind,
  draft,
  problem,
  onChange,
}: {
  kind: NodeTaskKind;
  draft: SpecDraft;
  problem: { problem: SpecProblem; field: SpecField } | undefined;
  onChange: (draft: SpecDraft) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const set = <K extends SpecField>(field: K, value: SpecDraft[K]) => onChange({ ...draft, [field]: value });
  // A field says what is wrong once something is typed in it; an empty required one only keeps the
  // task from being sent.
  const error = (field: SpecField) =>
    problem?.field === field && String(draft[field]).trim() !== '' ? (
      <span className="text-xs text-red">{t(`node_agents.spec.problem.${problem.problem}`)}</span>
    ) : null;

  const number = (field: SpecField, label: string, placeholder?: string) => (
    <FormField label={label}>
      <FormInput
        inputMode="numeric"
        value={String(draft[field])}
        placeholder={placeholder ?? t('node_agents.spec.node_default')}
        onChange={(event) => set(field, event.target.value as never)}
      />
      {error(field)}
    </FormField>
  );

  if (kind === 'autotracing_run') {
    return (
      <>
        <FormField label={t('node_agents.spec.rule')} required hint={t(`node_agents.spec.rule_hint.${draft.rule}`)}>
          <FormSelect
            value={draft.rule}
            onChange={(value) => set('rule', value as SpecDraft['rule'])}
            options={RULES.map((rule) => ({ value: rule, label: t(`node_agents.spec.rules.${rule}`) }))}
          />
        </FormField>
        {CONTAINER_RULES.includes(draft.rule) && (
          <FormField label={t('node_agents.spec.container_id')} hint={t('node_agents.spec.rule_container_hint')}>
            <FormInput
              value={draft.containerId}
              placeholder="c86df4740fab"
              className="font-code"
              onChange={(event) => set('containerId', event.target.value)}
            />
          </FormField>
        )}
      </>
    );
  }

  const durationField = (
    <FormField label={t('node_agents.spec.duration')} required hint={t('node_agents.spec.duration_hint')}>
      <FormInput
        inputMode="numeric"
        value={draft.durationSecs}
        onChange={(event) => set('durationSecs', event.target.value)}
      />
      {error('durationSecs')}
    </FormField>
  );

  const targetFields = (
    <>
      <FormField label={t('node_agents.spec.target')}>
        <FormSelect
          value={draft.target}
          onChange={(value) => set('target', value as SpecDraft['target'])}
          options={TARGETS.map((target) => ({ value: target, label: t(`node_agents.spec.targets.${target}`) }))}
        />
      </FormField>
      {draft.target === 'pid' && (
        <FormField label={t('node_agents.spec.pid')} required>
          <FormInput inputMode="numeric" value={draft.pid} onChange={(event) => set('pid', event.target.value)} />
          {error('pid')}
        </FormField>
      )}
      {draft.target === 'container' && (
        <FormField label={t('node_agents.spec.container_id')} required>
          <FormInput
            value={draft.containerId}
            placeholder="c86df4740fab"
            className="font-code"
            onChange={(event) => set('containerId', event.target.value)}
          />
          {error('containerId')}
        </FormField>
      )}
      <FormRow>
        <FormField label={t('node_agents.spec.mode')}>
          <FormSelect
            value={draft.mode}
            onChange={(value) => set('mode', value as SpecDraft['mode'])}
            options={PROFILE_MODES.map((mode) => ({ value: mode, label: t(`node_agents.spec.modes.${mode}`) }))}
          />
        </FormField>
        {number('frequencyHz', t('node_agents.spec.frequency'))}
      </FormRow>
    </>
  );

  if (kind === 'profile') {
    return (
      <>
        {durationField}
        {targetFields}
      </>
    );
  }

  const chooseTool = (tool: ToolName) => onChange({ ...draft, tool, durationSecs: defaultDuration(tool) });
  const streams = draft.tool === 'dropwatch' || draft.tool === 'tcpshark';

  return (
    <>
      <FormField label={t('node_agents.spec.tool')} required hint={t(`node_agents.spec.tool_hint.${draft.tool}`)}>
        <FormSelect
          value={draft.tool}
          onChange={(value) => chooseTool(value as ToolName)}
          options={TOOLS.map((tool) => ({ value: tool, label: tool }))}
        />
      </FormField>
      {durationField}

      {streams && (
        <>
          <FormField label={t('node_agents.spec.filter')} hint={t('node_agents.spec.filter_hint')}>
            <FormInput
              value={draft.filter}
              placeholder="tcp and port 80"
              className="font-code"
              onChange={(event) => set('filter', event.target.value)}
            />
          </FormField>
          {number('maxEventsPerSecond', t('node_agents.spec.max_events_per_second'))}
        </>
      )}

      {draft.tool === 'dropwatch' && (
        <>
          <FormField label={t('node_agents.spec.device_scope')}>
            <FormSelect
              value={draft.deviceScope}
              onChange={(value) => set('deviceScope', value as SpecDraft['deviceScope'])}
              options={DEVICE_SCOPES.map((scope) => ({ value: scope, label: t(`node_agents.spec.device_scopes.${scope}`) }))}
            />
          </FormField>
          {draft.deviceScope !== 'all' && (
            <FormField label={t('node_agents.spec.interfaces')} required hint={t('node_agents.spec.list_hint')}>
              <FormInput
                value={draft.devices}
                placeholder="eth0, eth1"
                className="font-code"
                onChange={(event) => set('devices', event.target.value)}
              />
              {error('devices')}
            </FormField>
          )}
        </>
      )}

      {draft.tool === 'tcpshark' && (
        <FormGroup label={t('node_agents.spec.enable_tlp')} hint={t('node_agents.spec.enable_tlp_hint')}>
          <Switch
            checked={draft.enableTlp}
            aria-label={t('node_agents.spec.enable_tlp')}
            onCheckedChange={(checked) => set('enableTlp', checked)}
          />
        </FormGroup>
      )}

      {draft.tool === 'iotracing' && (
        <>
          <FormField label={t('node_agents.spec.block_devices')} hint={t('node_agents.spec.block_devices_hint')}>
            <FormInput
              value={draft.devices}
              placeholder="259:3"
              className="font-code"
              onChange={(event) => set('devices', event.target.value)}
            />
            {error('devices')}
          </FormField>
          <FormRow>
            {number('maxStack', t('node_agents.spec.max_stack'))}
            {number('maxProcess', t('node_agents.spec.max_process'))}
          </FormRow>
          <FormRow>
            {number('maxFilesPerProcess', t('node_agents.spec.max_files_per_process'))}
            {number('scheduleThresholdMs', t('node_agents.spec.schedule_threshold_ms'))}
          </FormRow>
        </>
      )}

      {draft.tool === 'perf' && targetFields}
    </>
  );
}

import { Check, GitBranch, Settings2, TestTube2, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PipelineInput, ScheduledPipeline } from '@/api/pipelines';
import { DisabledControl } from '@/shell/DisabledControl';
import {
  FormField,
  FormInput,
  FormSelect,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';

import {
  PipelineGraphEditor,
  pipelineGraphFromPipeline,
  pipelineInputFromGraph,
  type PipelineGraphModel,
  type PipelineSignalType,
} from './PipelineGraph';

interface PipelineFormProps {
  formId: string;
  initial?: ScheduledPipeline | null;
  validationRequest?: number;
  disabled?: boolean;
  disabledReason?: string | undefined;
  enabledDisabled?: boolean;
  enabledDisabledReason?: string | undefined;
  onSubmit: (payload: PipelineInput) => void;
}

/**
 * Shared workbench body reused by /pipelines/new and /pipelines/:id/edit.
 * Persists exactly the fields exposed by `crates/api/src/http/routes/
 * scheduled_pipelines.rs::CreateReq`.
 */
export function PipelineForm({
  formId,
  initial,
  validationRequest = 0,
  disabled = false,
  disabledReason,
  enabledDisabled = false,
  enabledDisabledReason,
  onSubmit,
}: PipelineFormProps) {
  const { t } = useTranslation('pipelines');
  const [name, setName] = React.useState(initial?.name ?? '');
  const [cron, setCron] = React.useState(initial?.cron ?? 'every:5m');
  const [lookback, setLookback] = React.useState(String(initial?.lookback_secs ?? 300));
  const [enabled, setEnabled] = React.useState(initial?.enabled ?? true);
  const [graph, setGraph] = React.useState<PipelineGraphModel>(() =>
    pipelineGraphFromPipeline(initial, 'logs'),
  );

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (disabled) return;
    onSubmit(pipelineInputFromGraph({
      name,
      cron,
      lookbackSecs: Number(lookback) || 300,
      enabled,
      graph,
    }));
  };

  const changeSignalType = (signalType: PipelineSignalType) => {
    const defaults = pipelineGraphFromPipeline(null, signalType);
    setGraph((current) => ({
      ...defaults,
      transforms: current.transforms,
      retryPolicy: current.retryPolicy,
    }));
  };

  return (
    <form
      id={formId}
      onSubmit={submit}
      className="flex min-h-[calc(100vh-var(--topbar-h)-104px)] flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1"
      aria-disabled={disabled || undefined}
    >
      <div className="flex min-h-11 flex-wrap items-center justify-between gap-3 border-b border-bd-0 bg-bg-1 px-4 py-2">
        <div className="flex items-center gap-1 font-sans text-xs">
          <WorkspaceStage icon={Settings2} label={t('workspace.stage_identity')} complete />
          <span className="h-px w-5 bg-bd-1" aria-hidden />
          <WorkspaceStage icon={GitBranch} label={t('workspace.stage_graph')} active />
          <span className="h-px w-5 bg-bd-1" aria-hidden />
          <WorkspaceStage icon={TestTube2} label={t('workspace.stage_validate')} />
        </div>
        <div className="font-sans text-xs text-tx-3">{t('workspace.autosave_hint')}</div>
      </div>

      <div className="grid gap-3 border-b border-bd-0 bg-bg-2 px-4 py-3 md:grid-cols-2 xl:grid-cols-[minmax(240px,2fr)_160px_190px_150px_190px]">
        <FormField label={t('flows.form.name_label')} required>
          <FormInput
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="prod-app-logs"
            className="bg-bg-1"
            disabled={disabled}
            disabledReason={disabledReason}
            required
          />
        </FormField>
        <FormField label={t('drawer.fields.signal_type')} required>
          <FormSelect
            value={graph.signalType}
            onChange={(value) => changeSignalType(value as PipelineSignalType)}
            options={[
              { value: 'logs', label: t('filters.logs') },
              { value: 'metrics', label: t('filters.metrics') },
              { value: 'traces', label: t('filters.traces') },
            ]}
            className="bg-bg-1"
            disabled={disabled}
            disabledReason={disabledReason}
          />
        </FormField>
        <FormField label={t('flows.form.cron_label')} hint={t('flows.form.cron_hint')}>
          <FormInput
            value={cron}
            onChange={(e) => setCron(e.target.value)}
            className="bg-bg-1 font-mono text-xs"
            disabled={disabled}
            disabledReason={disabledReason}
            required
          />
        </FormField>
        <FormField label={t('flows.form.lookback_label')}>
          <FormInput
            value={lookback}
            onChange={(e) => setLookback(e.target.value)}
            type="number"
            className="bg-bg-1"
            disabled={disabled}
            disabledReason={disabledReason}
          />
        </FormField>
        <FormField label={t('flows.form.enabled_label')}>
          <DisabledControl
            disabled={disabled || enabledDisabled}
            reason={disabled ? disabledReason : enabledDisabledReason}
            className="w-full"
          >
            <span
              className={cn(
                'flex h-9 items-center gap-2 rounded-md border px-3 font-sans text-xs',
                disabled || enabledDisabled
                  ? 'cursor-not-allowed border-bd-0 bg-bg-3 text-tx-3'
                  : 'border-bd-1 bg-bg-1 text-tx-1',
              )}
            >
              <Checkbox
                checked={enabled}
                disabled={disabled || enabledDisabled}
                aria-disabled={disabled || enabledDisabled || undefined}
                onCheckedChange={(checked) => setEnabled(checked === true)}
              />
              <span>{t('flows.form.run_on_schedule')}</span>
            </span>
          </DisabledControl>
        </FormField>
      </div>

      <PipelineGraphEditor
        value={graph}
        onChange={setGraph}
        validationRequest={validationRequest}
        readOnly={disabled}
        readOnlyReason={disabledReason}
        defaultInspectorOpen
        className="min-h-[600px] flex-1 rounded-none border-0"
      />
    </form>
  );
}

function WorkspaceStage({
  icon: Icon,
  label,
  active,
  complete,
}: {
  icon: LucideIcon;
  label: React.ReactNode;
  active?: boolean;
  complete?: boolean;
}) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded px-2 py-1 font-strong',
        active ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-3',
        complete && 'text-green-soft',
      )}
    >
      {complete ? <Check className="h-3.5 w-3.5" /> : <Icon className="h-3.5 w-3.5" />}
      {label}
    </span>
  );
}

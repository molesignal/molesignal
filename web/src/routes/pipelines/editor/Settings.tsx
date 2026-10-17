import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Switch } from '@/shell/ui/switch';

import { IntervalField } from './IntervalField';
import type { PipelineResources } from './resources';
import type { Interval } from './schedule';
import { Segmented } from './Segmented';
import { StreamField } from './StreamField';
import { MAX_RETRIES, type PipelineSignalType, type RetrySettings } from '../PipelineGraph/model';

export type PipelineMode = 'scheduled' | 'realtime';

export interface PipelineSettingsValue {
  name: string;
  mode: PipelineMode;
  signalType: PipelineSignalType;
  every: Interval;
  lookback: Interval;
  enabled: boolean;
  delaySecs: number;
  retry: RetrySettings;
  deadLetterStream: string;
  dedupeField: string;
}

/** The waits for late events offered, in seconds. */
const DELAYS = [0, 30, 60, 120, 300, 600, 1_800, 3_600];

/**
 * What a pipeline is and when it runs: its name, the signal it carries,
 * whether it works on events as they arrive or on a schedule, and for a
 * schedule how often it runs, how far back its first run reads, how long it
 * leaves the latest events to arrive, and what it does when something fails.
 */
export function PipelineSettings({
  value,
  resources,
  onChange,
  onModeChange,
  onSignalChange,
  disabled = false,
  disabledReason,
  enabledDisabled = false,
  enabledDisabledReason,
}: {
  value: PipelineSettingsValue;
  resources: PipelineResources;
  onChange: (patch: Partial<PipelineSettingsValue>) => void;
  /** Changing the mode or the signal reshapes the flow, so each has its own handler. */
  onModeChange: (mode: PipelineMode) => void;
  onSignalChange: (signalType: PipelineSignalType) => void;
  disabled?: boolean;
  disabledReason?: string | undefined;
  enabledDisabled?: boolean;
  enabledDisabledReason?: string | undefined;
}) {
  const { t } = useTranslation('pipelines');
  const realtime = value.mode === 'realtime';
  const switchDisabled = disabled || enabledDisabled;

  return (
    <section
      data-pipeline-settings
      aria-label={t('settings.aria')}
      className={cn('px-[16px] py-[14px]', surfacePanelClass)}
    >
      <div className="grid gap-x-[16px] gap-y-[12px] lg:grid-cols-[minmax(0,1.6fr)_minmax(140px,0.7fr)_minmax(200px,0.9fr)]">
        <FormField label={t('flows.form.name_label')} required>
          <FormInput
            value={value.name}
            onChange={(event) => onChange({ name: event.target.value })}
            placeholder={t('settings.name_placeholder')}
            autoComplete="off"
            disabled={disabled}
            disabledReason={disabledReason}
            required
          />
        </FormField>
        <FormField label={t('drawer.fields.signal_type')}>
          <FormSelect
            value={value.signalType}
            onChange={(next) => onSignalChange(next as PipelineSignalType)}
            options={[
              { value: 'logs', label: t('filters.logs') },
              { value: 'metrics', label: t('filters.metrics') },
              { value: 'traces', label: t('filters.traces') },
            ]}
            disabled={disabled || realtime}
            disabledReason={disabled ? disabledReason : t('realtime.logs_only')}
          />
        </FormField>
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('realtime.mode')}</span>
          <Segmented
            label={t('realtime.mode')}
            value={value.mode}
            onChange={onModeChange}
            disabled={disabled}
            options={[
              { value: 'realtime', label: t('realtime.title') },
              { value: 'scheduled', label: t('realtime.scheduled') },
            ]}
          />
        </div>
      </div>

      <div className="mt-[12px] flex flex-wrap items-end gap-x-[16px] gap-y-[12px]">
        {realtime ? (
          <p className="m-0 flex min-h-9 min-w-0 flex-1 items-center font-sans text-sm leading-relaxed text-tx-2">
            {t('settings.realtime_note')}
          </p>
        ) : (
          <>
            <div className="flex min-w-0 flex-col gap-1.5">
              <span className={uiLabelClass}>{t('settings.every')}</span>
              <IntervalField
                label={t('settings.every')}
                value={value.every}
                onChange={(every) => onChange({ every })}
                disabled={disabled}
                disabledReason={disabledReason}
              />
            </div>
            <div className="flex min-w-0 flex-col gap-1.5">
              <span className={uiLabelClass}>{t('settings.lookback')}</span>
              <IntervalField
                label={t('settings.lookback')}
                value={value.lookback}
                onChange={(lookback) => onChange({ lookback })}
                disabled={disabled}
                disabledReason={disabledReason}
              />
            </div>
            <FormField label={t('settings.delay')} className="w-[148px]">
              <FormSelect
                value={String(value.delaySecs)}
                onChange={(next) => onChange({ delaySecs: Number(next) })}
                options={[...new Set([...DELAYS, value.delaySecs])]
                  .sort((a, b) => a - b)
                  .map((secs) => ({ value: String(secs), label: delayLabel(secs, t) }))}
                disabled={disabled}
                disabledReason={disabledReason}
              />
            </FormField>
          </>
        )}

        <DisabledControl
          disabled={switchDisabled}
          reason={disabled ? disabledReason : enabledDisabledReason}
          className="ml-auto"
        >
          <label
            className={cn(
              // The wrapper above is only there when the switch is disabled; the margin has to hold either way.
              'ml-auto flex h-9 items-center gap-[10px] font-sans text-sm',
              switchDisabled ? 'cursor-not-allowed text-tx-3' : 'cursor-pointer text-tx-1',
            )}
          >
            <Switch
              checked={value.enabled}
              disabled={switchDisabled}
              onCheckedChange={(enabled) => onChange({ enabled })}
            />
            {t(realtime ? 'realtime.enabled' : 'flows.form.run_on_schedule')}
          </label>
        </DisabledControl>
      </div>

      {!realtime && (
        <div
          data-pipeline-reliability
          className="mt-[12px] grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-[148px_148px_minmax(0,1fr)_minmax(0,1fr)]"
        >
          <FormField label={t('settings.run_retries')}>
            <FormSelect
              value={String(value.retry.runRetries)}
              onChange={(next) => onChange({ retry: { ...value.retry, runRetries: Number(next) } })}
              options={retryOptions(t)}
              disabled={disabled}
              disabledReason={disabledReason}
            />
          </FormField>
          <FormField label={t('settings.delivery_retries')}>
            <FormSelect
              value={String(value.retry.deliveryRetries)}
              onChange={(next) => onChange({ retry: { ...value.retry, deliveryRetries: Number(next) } })}
              options={retryOptions(t)}
              disabled={disabled}
              disabledReason={disabledReason}
            />
          </FormField>
          <StreamField
            label={t('settings.dead_letter')}
            value={value.deadLetterStream}
            // Dead letters are read in Logs whatever the pipeline carries.
            signalType="logs"
            resources={resources}
            creatable
            clearable
            placeholder={t('settings.dead_letter_none')}
            disabled={disabled}
            onChange={(deadLetterStream) => onChange({ deadLetterStream })}
          />
          <FormField label={t('settings.dedupe')}>
            <FormInput
              value={value.dedupeField}
              onChange={(event) => onChange({ dedupeField: event.target.value })}
              placeholder={t('settings.dedupe_none')}
              className="font-mono text-xs"
              autoComplete="off"
              spellCheck={false}
              disabled={disabled}
              disabledReason={disabledReason}
            />
          </FormField>
          <p className="col-span-full m-0 font-sans text-xs leading-relaxed text-tx-3">
            {t('settings.reliability_hint')}
          </p>
        </div>
      )}
    </section>
  );
}

function delayLabel(secs: number, t: TFunction<'pipelines'>): string {
  if (secs === 0) return t('settings.delay_none');
  if (secs % 3_600 === 0) return t('settings.delay_hours', { count: secs / 3_600 });
  if (secs % 60 === 0) return t('settings.delay_minutes', { count: secs / 60 });
  return t('settings.delay_seconds', { count: secs });
}

function retryOptions(t: TFunction<'pipelines'>) {
  return Array.from({ length: MAX_RETRIES + 1 }, (_, count) => ({
    value: String(count),
    label: count === 0 ? t('settings.retries_none') : t('settings.retries', { count }),
  }));
}

import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Switch } from '@/shell/ui/switch';

import { IntervalField } from './IntervalField';
import type { Interval } from './schedule';
import { Segmented } from './Segmented';
import type { PipelineSignalType } from '../PipelineGraph/model';

export type PipelineMode = 'scheduled' | 'realtime';

export interface PipelineSettingsValue {
  name: string;
  mode: PipelineMode;
  signalType: PipelineSignalType;
  every: Interval;
  lookback: Interval;
  enabled: boolean;
}

/**
 * What a pipeline is and when it runs: its name, the signal it carries,
 * whether it works on events as they arrive or on a schedule, and for a
 * schedule how often and how far back each run reads.
 */
export function PipelineSettings({
  value,
  onChange,
  onModeChange,
  onSignalChange,
  disabled = false,
  disabledReason,
  enabledDisabled = false,
  enabledDisabledReason,
}: {
  value: PipelineSettingsValue;
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
    </section>
  );
}

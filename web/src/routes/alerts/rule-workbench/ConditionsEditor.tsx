import { CircleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import type { AlertRuleKind, NoDataAction } from '@/types/alerting';

import { minutesFor, NO_DATA_ACTIONS, periodsFor, type RuleConditions } from './conditions';
import { judgesItself, withoutQuery } from './kinds';

/** When an incident recovers, and what a round without a value counts as. */
export function ConditionsEditor({
  value,
  kind,
  periodSecs,
  recoveryInvalid,
  onChange,
}: {
  value: RuleConditions;
  kind: AlertRuleKind;
  periodSecs: number;
  recoveryInvalid: boolean;
  onChange: (conditions: RuleConditions) => void;
}) {
  const { t } = useTranslation('alerts');
  const set = (patch: Partial<RuleConditions>) => onChange({ ...value, ...patch });

  const duration = (label: string, periods: number, onPeriods: (periods: number) => void) => (
    <FormField label={label}>
      <div className="flex items-center gap-2">
        <FormInput
          type="number"
          min={1}
          value={String(minutesFor(periods, periodSecs))}
          onChange={(event) => onPeriods(periodsFor(Number(event.target.value), periodSecs))}
          className="bg-[var(--control-surface)]"
        />
        <span className="shrink-0 text-xs text-tx-2">{t('workbench.threshold.minutes')}</span>
      </div>
    </FormField>
  );

  // The two settings stand side by side with their boxes on one line, however their hints wrap.
  return (
    <div data-alert-conditions>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="flex flex-col">
          <div className="mb-3">
            <div className={uiLabelClass}>{t('workbench.conditions.recovery')}</div>
            <div className="mt-0.5 text-xs text-tx-3">{t('workbench.conditions.recovery_hint')}</div>
          </div>
          <div className="mt-auto grid grid-cols-1 items-end gap-2 bg-bg-2 px-3 py-3 sm:grid-cols-[minmax(0,1fr)_150px]">
            {/* A rule that judges itself has no threshold to recover past, only rounds to wait. */}
            {!judgesItself(kind) && (
              <FormField label={t('workbench.conditions.recovery_threshold')}>
                <FormInput
                  type="number"
                  step="any"
                  value={value.recoveryThreshold}
                  placeholder={t('workbench.conditions.recovery_threshold_placeholder')}
                  onChange={(event) => set({ recoveryThreshold: event.target.value })}
                  className={cn('bg-[var(--control-surface)]', recoveryInvalid && 'border-red/60')}
                />
              </FormField>
            )}
            {duration(t('workbench.threshold.duration'), value.recoveryPeriods, (recoveryPeriods) =>
              set({ recoveryPeriods }),
            )}
          </div>
        </div>
        {/* A composite or heartbeat rule always has a value. */}
        {!withoutQuery(kind) && (
        <div className="flex flex-col">
          <div className="mb-3">
            <div className={uiLabelClass}>{t('workbench.conditions.no_data')}</div>
            <div className="mt-0.5 text-xs text-tx-3">{t('workbench.conditions.no_data_hint')}</div>
          </div>
          <div className="mt-auto grid grid-cols-1 items-end gap-2 bg-bg-2 px-3 py-3 sm:grid-cols-[minmax(0,1fr)_150px]">
            <FormField label={t('workbench.conditions.no_data_action')}>
              <FormSelect
                value={value.noDataAction}
                onChange={(next) => set({ noDataAction: next as NoDataAction })}
                options={NO_DATA_ACTIONS.map((action) => ({
                  value: action,
                  label: t(`workbench.conditions.no_data_actions.${action}`),
                }))}
                className="bg-[var(--control-surface)]"
              />
            </FormField>
            {value.noDataAction === 'alert' &&
              duration(t('workbench.conditions.no_data_duration'), value.noDataPeriods, (noDataPeriods) =>
                set({ noDataPeriods }),
              )}
          </div>
        </div>
        )}
      </div>
      {recoveryInvalid && (
        <div className="mt-2 flex items-start gap-2 rounded-md border border-red/30 bg-red-dim px-3 py-2 text-xs text-red-soft">
          <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          {t('workbench.errors.recovery_side')}
        </div>
      )}
    </div>
  );
}

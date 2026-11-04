import { CircleAlert, Plus, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import type { ComparisonOp, Severity, SeverityThreshold } from '@/types/alerting';

import { COMPARISON_LABEL } from '../alertRuleModel';

/** The thresholds of a rule, a band per severity: when it fires, how badly, and after how long. */
export function ThresholdEditor({
  thresholds,
  periodSecs,
  hasConflict,
  onChange,
}: {
  thresholds: SeverityThreshold[];
  periodSecs: number;
  hasConflict: boolean;
  onChange: (thresholds: SeverityThreshold[]) => void;
}) {
  const { t } = useTranslation('alerts');
  const update = (index: number, patch: Partial<SeverityThreshold>) =>
    onChange(
      thresholds.map((threshold, current) =>
        current === index ? { ...threshold, ...patch } : threshold,
      ),
    );
  return (
    <div>
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className={uiLabelClass}>{t('workbench.fields.thresholds')}</div>
          <div className="mt-0.5 text-xs text-tx-3">{t('workbench.hints.thresholds')}</div>
        </div>
        <ChromeButton
          size="sm"
          onClick={() =>
            onChange([
              ...thresholds,
              {
                severity: 'critical',
                operator: thresholds[0]?.operator ?? 'gt',
                threshold: thresholds[0]?.threshold ?? 0,
                forPeriods: Math.max(1, Math.ceil(120 / periodSecs)),
              },
            ])
          }
        >
          <Plus className="h-3.5 w-3.5" />
          {t('workbench.actions.add_threshold')}
        </ChromeButton>
      </div>
      <div className="mt-3 space-y-2">
        {thresholds.map((band, index) => {
          const durationMinutes = Math.max(
            1,
            Math.round((band.forPeriods * periodSecs) / 60),
          );
          return (
            <div
              key={`${band.severity}-${index}`}
              className="grid grid-cols-1 items-end gap-2 bg-bg-2 px-3 py-3 md:grid-cols-[130px_150px_minmax(100px,1fr)_150px_32px]"
            >
              <FormField label={t('workbench.threshold.severity')}>
                <FormSelect
                  value={band.severity}
                  onChange={(value) => update(index, { severity: value as Severity })}
                  options={[
                    { value: 'info', label: t('severity.info') },
                    { value: 'warning', label: t('severity.warning') },
                    { value: 'error', label: t('severity.error') },
                    { value: 'critical', label: t('severity.critical') },
                  ]}
                  className="bg-[var(--control-surface)]"
                />
              </FormField>
              <FormField label={t('workbench.threshold.operator')}>
                <FormSelect
                  value={band.operator}
                  onChange={(value) => update(index, { operator: value as ComparisonOp })}
                  options={(
                    ['gt', 'gte', 'lt', 'lte', 'eq', 'neq'] as ComparisonOp[]
                  ).map((operator) => ({
                    value: operator,
                    label: `${COMPARISON_LABEL[operator]} ${t(`workbench.operators.${operator}`)}`,
                  }))}
                  className="bg-[var(--control-surface)]"
                />
              </FormField>
              <FormField label={t('workbench.threshold.value')}>
                <FormInput
                  type="number"
                  step="any"
                  value={String(band.threshold)}
                  onChange={(event) =>
                    update(index, { threshold: Number(event.target.value) })
                  }
                  className="bg-[var(--control-surface)]"
                />
              </FormField>
              <FormField label={t('workbench.threshold.duration')}>
                <div className="flex items-center gap-2">
                  <FormInput
                    type="number"
                    min={1}
                    value={String(durationMinutes)}
                    onChange={(event) =>
                      update(index, {
                        forPeriods: Math.max(
                          1,
                          Math.ceil((Number(event.target.value) * 60) / periodSecs),
                        ),
                      })
                    }
                    className="bg-[var(--control-surface)]"
                  />
                  <span className="shrink-0 text-xs text-tx-2">
                    {t('workbench.threshold.minutes')}
                  </span>
                </div>
              </FormField>
              <button
                type="button"
                disabled={thresholds.length === 1}
                onClick={() => onChange(thresholds.filter((_, current) => current !== index))}
                className="grid h-9 w-8 place-items-center rounded-md text-tx-3 hover:bg-red-dim hover:text-red-soft disabled:cursor-not-allowed disabled:opacity-30"
                aria-label={t('workbench.actions.remove_threshold')}
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          );
        })}
      </div>
      {hasConflict && (
        <div className="mt-2 flex items-start gap-2 rounded-md border border-red/30 bg-red-dim px-3 py-2 text-xs text-red-soft">
          <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          {t('workbench.errors.threshold_conflict')}
        </div>
      )}
    </div>
  );
}

import { useTranslation } from 'react-i18next';

import { FormField, FormInput } from '@/shell/FormDrawer';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { DOWNSAMPLE_INTERVALS, MAX_RETENTION_DAYS, type StreamDraft } from './settingsDraft';
import { StreamSection, StreamToggleRow } from './Surfaces';

/**
 * How long a stream's data is kept and, for metrics, after how many days its
 * samples are kept as time buckets only. A stream without days of its own is
 * kept as long as the platform setting says.
 */
export function RetentionPanel({
  draft,
  effectiveRetentionDays,
  downsamples,
  onChange,
}: {
  draft: StreamDraft;
  effectiveRetentionDays: number;
  /** Downsampling is for metrics streams. */
  downsamples: boolean;
  onChange: (patch: Partial<StreamDraft>) => void;
}) {
  const { t } = useTranslation('streams');

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <StreamSection
        title={t('explore.retention.default_policy')}
        bodyClassName="space-y-4 pt-4"
      >
        <FormField
          label={t('explore.retention.data_retention_days')}
          hint={t('explore.retention.days_default_hint', {
            days: effectiveRetentionDays,
          })}
        >
          <FormInput
            type="number"
            min={1}
            max={MAX_RETENTION_DAYS}
            value={draft.retentionDays}
            onChange={(event) => onChange({ retentionDays: event.target.value })}
            placeholder={t('explore.retention.days_placeholder', {
              days: effectiveRetentionDays,
            })}
          />
        </FormField>
        <div className="bg-bg-2 px-3 py-2.5 font-sans text-xs leading-relaxed text-tx-2">
          {t('explore.retention.default_explanation')}
        </div>
      </StreamSection>

      {downsamples && (
        <StreamSection
          title={t('explore.retention.downsample')}
          bodyClassName="space-y-4 pt-4"
        >
          <StreamToggleRow
            title={t('explore.retention.downsample_toggle')}
            hint={t('explore.retention.downsample_hint')}
            checked={draft.downsample}
            onChange={(downsample) => onChange({ downsample })}
          />
          {draft.downsample && (
            <>
              <FormField
                label={t('explore.retention.downsample_after_days')}
                hint={t('explore.retention.downsample_after_days_hint')}
              >
                <FormInput
                  type="number"
                  min={1}
                  max={MAX_RETENTION_DAYS}
                  value={draft.downsampleAfterDays}
                  onChange={(event) =>
                    onChange({ downsampleAfterDays: event.target.value })
                  }
                />
              </FormField>
              <FormField label={t('explore.retention.downsample_interval')}>
                <FittedSelect
                  value={String(draft.downsampleInterval)}
                  onValueChange={(value) => onChange({ downsampleInterval: Number(value) })}
                  options={DOWNSAMPLE_INTERVALS.map((secs) => ({
                    value: String(secs),
                    label: t(`explore.retention.intervals.${secs}`),
                  }))}
                  maxWidth={200}
                  aria-label={t('explore.retention.downsample_interval')}
                />
              </FormField>
            </>
          )}
        </StreamSection>
      )}
    </div>
  );
}

import { CheckCircle2, ShieldCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { EffectiveFieldMaskingEntry } from '@/api/fieldMasking';
import type * as streamsApi from '@/api/streams';
import { AlgorithmEditor, algorithmSummary, defaultAlgorithm } from '@/features/fieldMasking/AlgorithmEditor';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormField, FormSelect } from '@/shell/FormDrawer';

import { indexOptionsFor, type FieldDraft, type FieldMaskingMode } from './model';
import { logicalFieldType } from '../model';

export function FieldEditDrawer({
  access,
  field,
  effectiveMasking,
  maskingSupported,
  onClose,
  onApply,
}: {
  access: ActionAccess;
  field: FieldDraft | null;
  effectiveMasking: EffectiveFieldMaskingEntry | null;
  maskingSupported: boolean;
  onClose: () => void;
  onApply: (field: FieldDraft) => void;
}) {
  const { t } = useTranslation('streams');
  const { t: settingsT } = useTranslation('settings-admin');
  const [editing, setEditing] = React.useState<FieldDraft | null>(field);

  React.useEffect(() => setEditing(field), [field]);

  return (
    <FormDrawer
      open={field !== null}
      onOpenChange={(open) => !open && onClose()}
      width={620}
      title={field ? t('explore.schema.drawer_title', { name: field.name }) : t('explore.schema.drawer_fallback')}
      subtitle={t('explore.schema.drawer_subtitle')}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('explore.schema.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            onClick={() => access.allowed && editing && onApply(editing)}
            disabled={access.disabled || !editing}
            disabledReason={access.reason}
          >
            {t('explore.schema.apply')}
          </ChromeButton>
        </>
      }
    >
      {editing ? (
        <fieldset
          disabled={access.disabled}
          aria-disabled={access.disabled || undefined}
          title={access.reason}
          className="contents disabled:cursor-not-allowed"
        >
          <div className="space-y-6">
            <div className="grid grid-cols-2 gap-3 rounded-lg border border-bd-0 bg-bg-2 p-4">
              <SummaryRow label={t('explore.schema.columns.name')} value={editing.name} />
              <SummaryRow label={t('explore.schema.columns.type')} value={t(`explore.schema.field_types.${logicalFieldType(editing.dataType)}`)} />
            </div>

            <div className="rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 font-sans text-xs leading-relaxed text-tx-1">
              {t('explore.changes.index_risk')}
            </div>

            <FormField label={t('explore.schema.columns.index_type')} hint={t('explore.schema.index_type_hint')}>
              <FormSelect
                value={editing.indexType}
                onChange={(value) => setEditing((current) => current ? {
                  ...current,
                  indexType: value as streamsApi.StreamIndexType,
                  indexed: value !== 'none',
                } : current)}
                options={indexOptionsFor(editing.dataType).map((option) => ({
                  value: option.value,
                  label: t(option.labelKey),
                }))}
              />
            </FormField>

            {maskingSupported ? (
              <section className="space-y-4 border-t border-bd-0 pt-5">
                <div>
                  <div className="font-sans text-sm font-semibold text-tx-0">{t('explore.schema.masking.title')}</div>
                  <div className="mt-1 font-sans text-xs leading-relaxed text-tx-3">{t('explore.schema.masking.description')}</div>
                </div>
                <FormField label={t('explore.schema.masking.mode')}>
                  <FormSelect
                    value={editing.masking_mode}
                    onChange={(value) => setEditing((current) => current ? {
                      ...current,
                      masking_mode: value as FieldMaskingMode,
                      masking_algorithm: value === 'custom' && current.masking_mode !== 'custom'
                        ? (effectiveMasking?.inheritedAlgorithm ?? defaultAlgorithm('full'))
                        : current.masking_algorithm,
                    } : current)}
                    options={(['inherit', 'custom', 'none'] as const).map((value) => ({
                      value,
                      label: t(`explore.schema.masking.modes.${value}`),
                    }))}
                  />
                </FormField>

                {editing.masking_mode === 'inherit' ? (
                  <div className="flex items-start gap-3 rounded-lg border border-indigo/25 bg-indigo-dim px-4 py-3">
                    <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-indigo-soft" />
                    <div>
                      <div className="font-sans text-sm font-semibold text-tx-0">
                        {effectiveMasking?.inheritedRuleName ?? t('explore.schema.masking.no_global_rule')}
                      </div>
                      <div className="mt-1 font-sans text-xs text-tx-2">
                        {effectiveMasking?.inheritedAlgorithm
                          ? algorithmSummary(effectiveMasking.inheritedAlgorithm, settingsT)
                          : t('explore.schema.masking.no_global_rule_hint')}
                      </div>
                    </div>
                  </div>
                ) : null}

                {editing.masking_mode === 'custom' ? (
                  <AlgorithmEditor
                    value={editing.masking_algorithm}
                    onChange={(algorithm) => setEditing((current) => current ? { ...current, masking_algorithm: algorithm } : current)}
                    t={settingsT}
                  />
                ) : null}

                {editing.masking_mode === 'none' ? (
                  <div className="rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 font-sans text-xs leading-relaxed text-tx-1">
                    {t('explore.schema.masking.explicit_none_hint')}
                  </div>
                ) : null}
              </section>
            ) : (
              <section className="space-y-2 border-t border-bd-0 pt-5">
                <div className="font-sans text-sm font-semibold text-tx-0">{t('explore.schema.masking.title')}</div>
                <div className="rounded-lg border border-bd-0 bg-bg-2 px-4 py-3 font-sans text-xs leading-relaxed text-tx-2">
                  {t('explore.schema.masking.metrics_disabled')}
                </div>
              </section>
            )}

            {editing.encrypted ? (
              <div className="flex items-start gap-3 rounded-lg border border-green/25 bg-green-dim px-4 py-3">
                <CheckCircle2 className="mt-0.5 h-4 w-4 text-[var(--green-ink)]" />
                <div>
                  <div className="font-sans text-sm font-semibold text-tx-0">{t('explore.schema.encrypted')}</div>
                  <div className="mt-1 font-sans text-xs text-tx-2">{t('explore.schema.encrypted_hint')}</div>
                </div>
              </div>
            ) : null}
          </div>
        </fieldset>
      ) : null}
    </FormDrawer>
  );
}

function SummaryRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="px-3 py-2">
      <div className="font-sans text-xs text-tx-3">{label}</div>
      <div className="mt-1 font-sans text-sm font-semibold text-tx-0">{value}</div>
    </div>
  );
}

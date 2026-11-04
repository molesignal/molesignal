import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as qualityApi from '@/api/agent/quality';
import type { GateMode, ScoringSettings } from '@/api/agent/quality';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { uiSectionTitleClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { QUALITY_KEY, useScoringSettings } from './state';

const GATES: readonly GateMode[] = ['warn', 'block', 'off'];

interface Draft {
  /** 0 to 100, as scores read. */
  threshold: string;
  sample: string;
  gate: GateMode;
}

function draftOf(settings: ScoringSettings): Draft {
  return {
    threshold: String(Math.round(settings.reviewThreshold * 100)),
    sample: String(settings.reviewSamplePercent),
    gate: settings.gate,
  };
}

/** A whole number from `min` to `max`, or null. */
function whole(value: string, min: number, max: number): number | null {
  const number = Number(value.trim());
  return value.trim() !== '' && Number.isInteger(number) && number >= min && number <= max ? number : null;
}

/**
 * How the organization's answers are reviewed and its prompts promoted: the
 * score below which an answer waits for review, how many others are drawn at
 * random, and what setting an organization prompt as the default asks for.
 */
export function ScoringSettingsForm() {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const { settings } = useScoringSettings();
  const [draft, setDraft] = React.useState<Draft | null>(null);

  React.useEffect(() => {
    if (settings.data) setDraft(draftOf(settings.data));
  }, [settings.data]);

  const save = useMutation({
    mutationFn: (input: Pick<ScoringSettings, 'reviewThreshold' | 'reviewSamplePercent' | 'gate'>) =>
      qualityApi.putSettings(input),
    onSuccess: () => {
      toast.success(t('quality.settings.saved'));
      void queryClient.invalidateQueries({ queryKey: QUALITY_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (settings.isPending) return <ProductState variant="loading" />;
  if (settings.isError) return <ProductState variant="error" error={settings.error} />;
  if (!draft) return null;

  const threshold = whole(draft.threshold, 0, 100);
  const sample = whole(draft.sample, 0, 100);
  const valid = threshold !== null && sample !== null;

  return (
    <section data-scoring-settings aria-labelledby="scoring-settings" className="flex flex-col gap-3">
      <div>
        <h3 id="scoring-settings" className={`m-0 ${uiSectionTitleClass}`}>
          {t('quality.settings.title')}
        </h3>
        <p className="m-0 mt-0.5 font-sans text-xs text-tx-2">{t('quality.settings.description')}</p>
      </div>
      <div className="grid gap-4 md:grid-cols-3">
        <FormField label={t('quality.settings.threshold')} hint={t('quality.settings.threshold_hint')}>
          <FormInput
            type="number"
            min={0}
            max={100}
            value={draft.threshold}
            onChange={(event) => setDraft({ ...draft, threshold: event.target.value })}
          />
        </FormField>
        <FormField label={t('quality.settings.sample')} hint={t('quality.settings.sample_hint')}>
          <FormInput
            type="number"
            min={0}
            max={100}
            value={draft.sample}
            onChange={(event) => setDraft({ ...draft, sample: event.target.value })}
          />
        </FormField>
        <FormField label={t('quality.settings.gate')} hint={t(`quality.settings.gate_hint.${draft.gate}`)}>
          <FormSelect
            ariaLabel={t('quality.settings.gate')}
            value={draft.gate}
            onChange={(gate) => setDraft({ ...draft, gate: gate as GateMode })}
            options={GATES.map((gate) => ({ value: gate, label: t(`quality.settings.gates.${gate}`) }))}
          />
        </FormField>
      </div>
      <div>
        <Button
          size="sm"
          disabled={!valid || save.isPending}
          onClick={() =>
            threshold !== null &&
            sample !== null &&
            save.mutate({ reviewThreshold: threshold / 100, reviewSamplePercent: sample, gate: draft.gate })
          }
        >
          {t('quality.settings.save')}
        </Button>
      </div>
    </section>
  );
}

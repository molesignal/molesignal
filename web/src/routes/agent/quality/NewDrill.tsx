import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as drillsApi from '@/api/agent/drills';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import {
  FormDrawer,
  FormField,
  FormGroup,
  FormInput,
  FormRadio,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { REFERENCE_MAX, referenceProblem } from './drillsModel';
import { DRILLS_KEY } from './state';

const FORM_ID = 'agent-new-drill';
/** The choice for an incident that is not among the candidates. */
const OTHER = '__other__';

/**
 * A new drill: an incident resolved in the last 180 days whose postmortem
 * states a root cause, or another incident with a root cause written here. The
 * postmortem's root cause is the reference unless one is written instead.
 */
export function NewDrill({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const candidates = useQuery({
    queryKey: [...DRILLS_KEY, 'candidates'],
    queryFn: drillsApi.listCandidates,
    retry: false,
  });
  const [choice, setChoice] = React.useState('');
  const [otherId, setOtherId] = React.useState('');
  const [own, setOwn] = React.useState(false);
  const [reference, setReference] = React.useState('');

  const open = (candidates.data ?? []).filter((candidate) => !candidate.drilled);
  const picked = open.find((candidate) => candidate.incidentId === choice) ?? null;
  const other = choice === OTHER || (candidates.isSuccess && open.length === 0);
  const incidentId = other ? otherId.trim() : (picked?.incidentId ?? '');
  const writes = other || own;
  const problem = !incidentId ? 'incident' : writes ? referenceProblem(reference) : null;

  const save = useMutation({
    mutationFn: () =>
      drillsApi.createDrill({
        incidentId,
        ...(writes ? { reference: reference.trim() } : {}),
      }),
    onSuccess: (drill) => {
      toast.success(t('quality.drills.created', { name: drill.title }));
      void queryClient.invalidateQueries({ queryKey: DRILLS_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const options = [
    ...open.map((candidate) => ({
      value: candidate.incidentId,
      label: candidate.summary,
      hint: t('quality.drills.candidate_hint', {
        resolved: candidate.resolvedAt ? formatMicrosActive(candidate.resolvedAt) : '—',
        cause: candidate.rootCause,
      }),
    })),
    { value: OTHER, label: t('quality.drills.other_incident'), hint: t('quality.drills.other_incident_hint') },
  ];

  return (
    <FormDrawer
      open
      onOpenChange={(next) => !next && onClose()}
      title={t('quality.drills.new_title')}
      subtitle={t('quality.drills.new_subtitle')}
      width={640}
      footer={
        <FormSubmitFooter
          formId={FORM_ID}
          busy={save.isPending}
          invalid={problem !== null}
          onCancel={onClose}
          submitLabel={t('quality.drills.create')}
        />
      }
    >
      <form
        id={FORM_ID}
        data-new-drill=""
        onSubmit={(event) => {
          event.preventDefault();
          event.stopPropagation();
          if (problem === null && !save.isPending) save.mutate();
        }}
      >
        <FormSection title={t('quality.drills.incident')} description={t('quality.drills.incident_hint')}>
          {candidates.isPending ? (
            <ProductState variant="loading" />
          ) : candidates.isError ? (
            <ProductState variant="error" error={candidates.error} />
          ) : (
            <>
              {open.length === 0 && (
                <p className="m-0 font-sans text-xs text-tx-2">{t('quality.drills.no_candidates')}</p>
              )}
              {open.length > 0 && (
                <FormGroup label={t('quality.drills.candidates')}>
                  <FormRadio
                    options={options}
                    value={choice}
                    onChange={(value) => setChoice(value)}
                  />
                </FormGroup>
              )}
              {other && (
                <FormField label={t('quality.drills.incident_id')} required>
                  <FormInput
                    value={otherId}
                    placeholder={t('quality.drills.incident_id_placeholder')}
                    onChange={(event) => setOtherId(event.target.value)}
                  />
                </FormField>
              )}
            </>
          )}
        </FormSection>
        <FormSection title={t('quality.drills.reference')} description={t('quality.drills.reference_hint')}>
          {!other && picked && (
            <label className="flex items-center gap-2 font-sans text-xs text-tx-1">
              <Switch checked={own} onCheckedChange={setOwn} />
              {t('quality.drills.own_reference')}
            </label>
          )}
          {writes ? (
            <FormField label={t('quality.drills.reference_text')} required>
              <FormTextarea
                value={reference}
                rows={6}
                maxLength={REFERENCE_MAX}
                onChange={(event) => setReference(event.target.value)}
              />
            </FormField>
          ) : picked ? (
            <p data-new-drill-reference="" className="m-0 whitespace-pre-wrap font-sans text-xs text-tx-1">
              {picked.rootCause}
            </p>
          ) : (
            <p className="m-0 font-sans text-xs text-tx-3">{t('quality.drills.pick_first')}</p>
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}

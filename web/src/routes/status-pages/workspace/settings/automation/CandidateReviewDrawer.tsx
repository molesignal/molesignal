import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, RotateCcw, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type {
  AutomationCandidateState,
  IncidentImpact,
  StatusPageComponent,
  StatusPageIncident,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton, Pill, type PillTone } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

export function CandidateReviewDrawer({
  pageId,
  candidateId,
  components,
  canPublish,
  publishDisabledReason,
  onClose,
}: {
  pageId: string;
  candidateId: string | null;
  components: StatusPageComponent[];
  canPublish: boolean;
  publishDisabledReason?: string | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const detail = useQuery({
    queryKey: ['status-pages', pageId, 'automation', 'candidate', candidateId],
    queryFn: () => statusPagesApi.getAutomationCandidateDetail(pageId, candidateId ?? ''),
    enabled: Boolean(candidateId),
  });
  const candidate = detail.data?.candidate;
  const event = useQuery({
    queryKey: ['status-pages', pageId, 'event', candidate?.status_incident_id],
    queryFn: () => statusPagesApi.getEvent(pageId, candidate?.status_incident_id ?? ''),
    enabled: Boolean(candidate?.status_incident_id),
  });
  const [draft, setDraft] = React.useState<CandidateDraft | null>(null);
  const [note, setNote] = React.useState('');
  React.useEffect(() => {
    if (event.data) setDraft(candidateDraft(event.data));
  }, [event.data]);
  React.useEffect(() => setNote(''), [candidateId]);

  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'automation'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'events'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId, 'snapshot'] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages', 'automation', 'pending'] }),
    ]);
  };
  const decision = useMutation({
    mutationFn: async (approve: boolean) => {
      if (!candidate) throw new Error('automation Candidate is unavailable');
      if (approve) {
        if (!draft) throw new Error('automation draft is unavailable');
        const decisionNote = note.trim();
        return statusPagesApi.approveAutomationCandidate(pageId, candidate.id, {
          title: draft.title.trim(),
          impact: draft.impact,
          message: draft.message.trim(),
          component_ids: draft.componentIds,
          ...(decisionNote ? { note: decisionNote } : {}),
        });
      }
      return statusPagesApi.rejectAutomationCandidate(pageId, candidate.id, note.trim());
    },
    onSuccess: async (_, approve) => {
      toast.success(t(approve ? 'automation.toast.approved' : 'automation.toast.rejected'));
      await invalidate();
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const retry = useMutation({
    mutationFn: () => statusPagesApi.retryAutomationCandidate(pageId, candidate?.id ?? ''),
    onSuccess: async () => {
      toast.success(t('automation.toast.retried'));
      await invalidate();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const approvalInvalid = !draft
    || !draft.title.trim()
    || !draft.message.trim()
    || draft.componentIds.length === 0;
  const missingDraft = candidate?.state === 'pending_approval' && !candidate.status_incident_id;
  const draftLoadFailed = candidate?.state === 'pending_approval' && event.isError;
  const draftProblem = missingDraft
    ? t('automation.review.missing_draft')
    : draftLoadFailed
      ? t('automation.review.draft_load_failed')
      : undefined;
  const approvalDisabledReason = !canPublish
    ? publishDisabledReason
    : missingDraft
      ? t('automation.review.missing_draft')
      : draftLoadFailed
        ? t('automation.review.draft_load_failed')
        : !draft
          ? t('automation.review.draft_loading')
          : approvalInvalid
            ? t('automation.review.complete_required_fields')
            : decision.isPending
              ? t('automation.review.decision_pending')
              : undefined;
  const rejectionDisabledReason = !canPublish
    ? publishDisabledReason
    : !note.trim()
      ? t('automation.review.rejection_note_required')
      : decision.isPending
        ? t('automation.review.decision_pending')
        : undefined;
  const footer = candidate?.state === 'pending_approval' ? (
    <>
      <ChromeButton
        disabled={Boolean(rejectionDisabledReason)}
        disabledReason={rejectionDisabledReason}
        onClick={() => decision.mutate(false)}
      >
        <X className="h-3.5 w-3.5" />
        {t('automation.actions.reject')}
      </ChromeButton>
      <ChromeButton
        variant="primary"
        disabled={Boolean(approvalDisabledReason)}
        disabledReason={approvalDisabledReason}
        onClick={() => decision.mutate(true)}
      >
        <Check className="h-3.5 w-3.5" />
        {t('automation.actions.approve_publish')}
      </ChromeButton>
    </>
  ) : candidate?.state === 'failed' ? (
    <ChromeButton
      variant="primary"
      disabled={!canPublish || retry.isPending}
      onClick={() => retry.mutate()}
    >
      <RotateCcw className="h-3.5 w-3.5" />
      {t('automation.actions.retry')}
    </ChromeButton>
  ) : undefined;

  return (
    <FormDrawer
      open={Boolean(candidateId)}
      onOpenChange={(open) => !open && onClose()}
      title={candidate?.title ?? t('automation.review.title')}
      subtitle={t('automation.review.subtitle')}
      width={820}
      footer={footer}
    >
      {detail.isLoading ? (
        <ProductState variant="loading" />
      ) : detail.isError || !candidate ? (
        <ProductState variant="error" error={detail.error} />
      ) : (
        <div className="space-y-7">
          <div className="grid gap-3 sm:grid-cols-3">
            <Fact label={t('columns.status')}>
              <Pill tone={candidateTone(candidate.state)}>{t(`automation.candidate.${candidate.state}`)}</Pill>
            </Fact>
            <Fact label={t('columns.impact')}>
              <span>{t(`impact.${event.data?.impact ?? candidate.impact}`)}</span>
            </Fact>
            <Fact label={t('automation.review.due_at')}>
              <span className="tabular-nums">{formatMicrosActive(candidate.due_at)}</span>
            </Fact>
          </div>

          {draftProblem && (
            <div
              role="alert"
              className="rounded-md border border-red/30 bg-red-dim px-4 py-3 text-sm leading-relaxed text-red-soft"
            >
              {draftProblem}
            </div>
          )}

          {candidate.state === 'pending_approval' && draft ? (
            <FormSection title={t('automation.review.public_copy')} description={t('automation.review.public_copy_hint')}>
              <FormField label={t('columns.title')} required>
                <FormInput
                  value={draft.title}
                  maxLength={200}
                  className="text-base sm:text-sm"
                  onChange={(change) => setDraft({ ...draft, title: change.currentTarget.value })}
                />
              </FormField>
              <FormField label={t('columns.impact')}>
                <FormSelect
                  value={draft.impact}
                  onChange={(impact) => setDraft({ ...draft, impact: impact as IncidentImpact })}
                  options={(['minor', 'major', 'critical'] as const).map((impact) => ({
                    value: impact,
                    label: t(`impact.${impact}`),
                  }))}
                />
              </FormField>
              <FormField label={t('fields.message')} required>
                <FormTextarea
                  value={draft.message}
                  maxLength={4_000}
                  className="text-base sm:text-sm"
                  onChange={(change) => setDraft({ ...draft, message: change.currentTarget.value })}
                />
              </FormField>
              <ComponentChecklist components={components} value={draft.componentIds} onChange={(componentIds) => setDraft({ ...draft, componentIds })} />
              <FormField label={t('automation.review.note')} hint={t('automation.review.note_hint')}>
                <FormTextarea
                  value={note}
                  maxLength={500}
                  className="min-h-20 text-base sm:text-sm"
                  onChange={(change) => setNote(change.currentTarget.value)}
                />
              </FormField>
            </FormSection>
          ) : (
            <FormSection title={t('automation.review.public_copy')}>
              <div className="rounded-md border border-bd-0 bg-bg-2 px-4 py-3">
                <div className="text-sm font-strong text-tx-0">{event.data?.title ?? candidate.title}</div>
                <p className="mt-2 whitespace-pre-wrap text-sm leading-relaxed text-tx-2">
                  {event.data?.draft_message ?? event.data?.updates.at(-1)?.message ?? candidate.message}
                </p>
              </div>
            </FormSection>
          )}

          {candidate.last_error && (
            <FormSection title={t('automation.review.failure')}>
              <div className="rounded-md border border-red/30 bg-red-dim px-4 py-3 font-code text-xs leading-relaxed text-red-soft">
                {candidate.last_error}
              </div>
            </FormSection>
          )}

          <FormSection title={t('automation.review.sources')} description={t('automation.review.sources_hint')}>
            <div className="overflow-hidden rounded-md border border-bd-0">
              {(detail.data?.sources ?? []).map((source) => (
                <div key={`${source.source_kind}:${source.source_instance_id}`} className="border-b border-bd-0 px-4 py-3 last:border-b-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <Pill tone={source.active && !source.muted ? 'green' : 'dim'}>
                      {source.active && !source.muted ? t('automation.review.active') : t('automation.review.inactive')}
                    </Pill>
                    <span className="text-xs font-strong text-tx-0">{t(`automation.source.${source.source_kind}`)}</span>
                    <span className="font-code text-xs text-tx-3">{source.source_instance_id}</span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {Object.entries(source.labels).map(([key, value]) => (
                      <span key={key} className="rounded bg-bg-3 px-2 py-1 font-code text-xs text-tx-2">{key}={value}</span>
                    ))}
                  </div>
                  <div className="mt-2 text-xs tabular-nums text-tx-3">{formatMicrosActive(source.observed_at)}</div>
                </div>
              ))}
            </div>
          </FormSection>

          <FormSection title={t('automation.review.activity')}>
            {(detail.data?.work_items.length ?? 0) > 0 && (
              <div className="mb-4 overflow-hidden rounded-md border border-bd-0">
                {(detail.data?.work_items ?? []).map((item) => (
                  <div key={item.id} className="flex flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-2.5 last:border-b-0">
                    <Pill tone={item.status === 'dead_letter' ? 'red' : item.status === 'completed' ? 'green' : 'yellow'}>
                      {t(`automation.work_status.${item.status}`)}
                    </Pill>
                    <span className="text-xs font-strong text-tx-1">{t(`automation.work_kind.${item.kind}`)}</span>
                    <span className="text-xs text-tx-3">{t('automation.review.attempts', { count: item.attempts })}</span>
                    <span className="ml-auto text-xs tabular-nums text-tx-3">{formatMicrosActive(item.updated_at)}</span>
                  </div>
                ))}
              </div>
            )}
            <div className="space-y-2">
              {(detail.data?.actions ?? []).map((action) => (
                <div key={action.id} className="flex gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
                  <span className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full bg-indigo" />
                  <div className="min-w-0 flex-1">
                    <div className="text-xs font-strong text-tx-1">{t(`automation.action.${action.action}`, { defaultValue: action.action })}</div>
                    {action.note && <div className="mt-1 text-xs leading-relaxed text-tx-3">{action.note}</div>}
                  </div>
                  <span className="shrink-0 text-xs tabular-nums text-tx-3">{formatMicrosActive(action.created_at)}</span>
                </div>
              ))}
            </div>
          </FormSection>
        </div>
      )}
    </FormDrawer>
  );
}

interface CandidateDraft {
  title: string;
  impact: IncidentImpact;
  message: string;
  componentIds: string[];
}

function candidateDraft(event: StatusPageIncident): CandidateDraft {
  return {
    title: event.title,
    impact: event.impact,
    message: event.draft_message ?? event.updates.at(-1)?.message ?? '',
    componentIds: event.component_ids,
  };
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
      <div className="text-xs text-tx-3">{label}</div>
      <div className="mt-1.5 text-sm text-tx-1">{children}</div>
    </div>
  );
}

function ComponentChecklist({
  components,
  value,
  onChange,
}: {
  components: StatusPageComponent[];
  value: string[];
  onChange: (value: string[]) => void;
}) {
  const { t } = useTranslation('status-pages');
  return (
    <FormField label={t('fields.affected_components')} required>
      <div className="grid gap-2 sm:grid-cols-2">
        {components.filter((component) => component.lifecycle === 'active').map((component) => (
          <label key={component.id} className="flex min-h-11 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1 hover:bg-bg-3">
            <input
              type="checkbox"
              checked={value.includes(component.id)}
              onChange={(change) => onChange(change.currentTarget.checked
                ? [...value, component.id]
                : value.filter((id) => id !== component.id))}
              className="h-4 w-4 accent-indigo"
            />
            <span>{component.name}</span>
          </label>
        ))}
      </div>
    </FormField>
  );
}

function candidateTone(state: AutomationCandidateState): PillTone {
  if (state === 'published' || state === 'resolved') return 'green';
  if (state === 'failed') return 'red';
  if (state === 'pending_approval' || state === 'approved' || state === 'delayed') return 'yellow';
  return 'dim';
}

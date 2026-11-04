import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import type { AgentPrompt } from '@/api/agent/prompts';
import * as qualityApi from '@/api/agent/quality';
import type { PromptCheck, PromptGate } from '@/api/agent/quality';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

import { formatScore } from '../../quality/model';

export const PROMPT_GATE_KEY = ['agent', 'prompt-gate'] as const;

function checking(gate: PromptGate | undefined): boolean {
  return (
    Boolean(gate?.current) &&
    (gate?.check?.status === 'queued' || gate?.check?.status === 'running')
  );
}

/**
 * Setting an organization prompt as the default, when the organization checks
 * it first: what the last check found, a way to run one, and — when the
 * organization lets it be confirmed — a way to set it anyway.
 */
export function PromotionDialog({
  prompt,
  onClose,
  onSet,
}: {
  prompt: AgentPrompt;
  onClose: () => void;
  /** Set it as the default; `confirm` when the person chose to despite the check. */
  onSet: (confirm: boolean) => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const gate = useQuery({
    queryKey: [...PROMPT_GATE_KEY, prompt.id],
    queryFn: () => qualityApi.promptGate(prompt.id),
    refetchInterval: (query) => (checking(query.state.data) ? 3_000 : false),
  });
  const start = useMutation({
    mutationFn: () => qualityApi.startPromptCheck(prompt.id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: [...PROMPT_GATE_KEY, prompt.id] }),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const set = useMutation({
    mutationFn: (confirm: boolean) => onSet(confirm),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const data = gate.data;
  const check = data?.current ? data.check : null;
  const running = checking(data);
  const canCheck = data?.reason !== 'prompt_check_unavailable' && !running;
  const busy = start.isPending || set.isPending;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-[min(640px,calc(100vw-24px))] max-w-none" data-prompt-promotion={prompt.id}>
        <DialogHeader>
          <DialogTitle>{t('settings.prompts.promotion.title', { name: prompt.name })}</DialogTitle>
          <DialogDescription>{t('settings.prompts.promotion.description')}</DialogDescription>
        </DialogHeader>
        <div className="flex min-w-0 flex-col gap-3 font-sans text-xs leading-relaxed text-tx-1">
          {data?.reason && !running && (
            <p data-prompt-gate-reason={data.reason} className="m-0">
              {t(`settings.prompts.promotion.reasons.${data.reason}`, {
                mode: t(`settings.prompts.promotion.modes.${data.mode}`),
              })}
            </p>
          )}
          {running && check && <Progress check={check} />}
          {check?.status === 'failed' && (
            <p className="m-0 text-[var(--red-ink)]">
              {t('settings.prompts.promotion.failed', { error: check.error ?? '' })}
            </p>
          )}
          {check?.status === 'done' && <Result check={check} />}
        </div>
        <DialogFooter>
          {canCheck && data?.decision !== 'allow' && (
            <ChromeButton className="mr-auto" disabled={busy} onClick={() => start.mutate()}>
              {check ? t('settings.prompts.promotion.check_again') : t('settings.prompts.promotion.check')}
            </ChromeButton>
          )}
          <ChromeButton onClick={onClose}>{t('common.cancel')}</ChromeButton>
          {data?.decision === 'allow' && (
            <ChromeButton variant="primary" disabled={busy} onClick={() => set.mutate(false)}>
              {t('settings.prompts.set_default')}
            </ChromeButton>
          )}
          {data?.decision === 'confirm' && !running && (
            <ChromeButton variant="primary" disabled={busy} onClick={() => set.mutate(true)}>
              {t('settings.prompts.promotion.set_anyway')}
            </ChromeButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Progress({ check }: { check: PromptCheck }) {
  const { t } = useTranslation('agent');
  const done = check.samples.filter(
    (sample) => sample.error !== undefined || (sample.baseline !== undefined && sample.candidate !== undefined),
  ).length;

  return (
    <p className="m-0 text-tx-2" role="status" data-prompt-check-progress>
      {check.samples.length === 0
        ? t('settings.prompts.promotion.picking')
        : t('settings.prompts.promotion.progress', { done, total: check.samples.length })}
    </p>
  );
}

function Result({ check }: { check: PromptCheck }) {
  const { t } = useTranslation('agent');
  const scored = check.samples.filter(
    (sample) => sample.baseline !== undefined && sample.candidate !== undefined,
  );

  return (
    <div className="flex flex-col gap-2" data-prompt-check-verdict={check.verdict}>
      <p className="m-0">
        {t(`settings.prompts.promotion.verdicts.${check.verdict ?? 'inconclusive'}`, {
          baseline: formatScore(check.baselineMean),
          candidate: formatScore(check.candidateMean),
          count: scored.length,
        })}
      </p>
      {scored.length > 0 && (
        <Table className="text-xs">
          <TableHeader>
            <TableRow>
              <TableHead className="w-[52%]">{t('settings.prompts.promotion.columns.sample')}</TableHead>
              <TableHead className="text-right">{t('settings.prompts.promotion.columns.baseline')}</TableHead>
              <TableHead className="text-right">{t('settings.prompts.promotion.columns.candidate')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {scored.map((sample) => (
              <TableRow key={`${sample.targetKind}:${sample.targetId}`} className="align-top">
                <TableCell className="whitespace-normal">
                  <span className="text-tx-1">
                    {t(`settings.prompts.promotion.kinds.${sample.targetKind}`)}
                  </span>
                  {sample.regression && (
                    <span className="ml-1.5 text-[var(--orange-ink)]">
                      {t('settings.prompts.promotion.regression')}
                    </span>
                  )}
                  {sample.candidateReason && (
                    <span className="mt-0.5 block text-tx-2">{sample.candidateReason}</span>
                  )}
                </TableCell>
                <TableCell className="text-right">{formatScore(sample.baseline)}</TableCell>
                <TableCell className="text-right text-tx-0">{formatScore(sample.candidate)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  );
}

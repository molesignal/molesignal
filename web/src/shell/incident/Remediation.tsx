import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Wrench } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as agentApi from '@/api/agent/automations';
import * as remediationApi from '@/api/remediation';
import type { RemediationAction, RemediationRun, RemediationRunStatus } from '@/api/remediation';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormField, FormInput, FormSelect, FormTextarea } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import type { Incident } from '@/types/alerting';

import { IncidentSection } from './Section';

const STATUS_INK: Record<RemediationRunStatus, string> = {
  awaiting_approval: 'text-orange',
  running: 'text-blue',
  succeeded: 'text-green',
  verifying: 'text-blue',
  verified: 'text-green',
  failed: 'text-red',
  verification_failed: 'text-red',
  declined: 'text-tx-3',
};

/** What can be done about the incident, set up beforehand, and what was started: each run waits for approval. */
export function IncidentRemediation({ incident, surface }: { incident: Incident; surface: boolean }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const { nameOf } = useUsers();
  const currentUserId = useAuthStore((state) => state.ctx?.userId ?? '');
  const startAccess = useActionAccess({ permission: 'alerts.incidents.remediate' });
  const approveAccess = useActionAccess({ permission: 'agent.approve' });
  const manageAccess = useActionAccess({ permission: 'alerts.remediation.manage' });
  const [starting, setStarting] = React.useState<RemediationAction | null>(null);
  const key = ['incident-remediations', incident.id];
  const remediations = useQuery({
    queryKey: key,
    queryFn: () => remediationApi.incidentRemediations(incident.id),
    retry: false,
  });
  const review = useMutation({
    mutationFn: ({ run, approve }: { run: RemediationRun; approve: boolean }) =>
      agentApi.reviewApproval(run.approvalId, approve, ''),
    onSuccess: (_, { approve }) => {
      toast.success(t(approve ? 'remediation.run.approved' : 'remediation.run.rejected'));
      void queryClient.invalidateQueries({ queryKey: key });
      void queryClient.invalidateQueries({ queryKey: ['incident-timeline', incident.id] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const actions = remediations.data?.actions ?? [];
  const runs = remediations.data?.runs ?? [];
  const closed = incident.status === 'resolved' || incident.status === 'closed';

  return (
    <IncidentSection title={t('drawer.incident_sections.remediation')} icon={<Wrench className="h-3.5 w-3.5" />} surface={surface}>
      <div data-incident-remediation className="flex flex-col gap-3 font-sans text-xs">
        {actions.length === 0 ? (
          <p className="text-tx-2">
            {t('remediation.run.none')}
            {manageAccess.allowed && (
              <>
                {' '}
                <Link to="/alerts/remediation" className="text-indigo-soft underline-offset-2 hover:underline">
                  {t('remediation.run.set_up')}
                </Link>
              </>
            )}
          </p>
        ) : (
          <ul className="flex flex-col gap-2">
            {actions.map((action) => (
              <li key={action.id} data-remediation-action={action.id} className="flex min-w-0 items-center gap-3">
                <div className="min-w-0 flex-1">
                  <span className="block truncate text-sm text-tx-0">{action.name}</span>
                  {action.description && <span className="block truncate text-tx-2">{action.description}</span>}
                </div>
                <ChromeButton
                  size="sm"
                  disabled={startAccess.disabled || closed}
                  disabledReason={closed ? t('remediation.run.closed') : startAccess.reason}
                  onClick={() => setStarting(action)}
                >
                  {t('remediation.run.start')}
                </ChromeButton>
              </li>
            ))}
          </ul>
        )}

        {runs.length > 0 && (
          <ul data-remediation-runs className="flex flex-col divide-y divide-bd-0">
            {runs.map((run) => {
              const canReview =
                run.status === 'awaiting_approval' && approveAccess.allowed && run.requestedBy !== currentUserId;
              return (
                <li key={run.id} data-remediation-run={run.id} className="flex min-w-0 items-start gap-3 py-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex min-w-0 items-baseline gap-2">
                      <span className="truncate text-sm text-tx-0">{run.actionName}</span>
                      <span className={cn('shrink-0 font-strong', STATUS_INK[run.status])}>
                        {t(`remediation.run.status.${run.status}`)}
                      </span>
                    </div>
                    <p className="mt-0.5 text-tx-2">
                      {[
                        formatMicrosActive(run.createdAt, false),
                        t('remediation.run.by', { name: nameOf(run.requestedBy) }),
                        Object.entries(run.inputs)
                          .map(([name, value]) => `${name}=${value}`)
                          .join(', ') || null,
                      ]
                        .filter(Boolean)
                        .join(' · ')}
                    </p>
                    {(run.output || run.verification) && (
                      <p className="mt-0.5 line-clamp-2 break-all text-tx-1">
                        {[run.output, run.verification].filter(Boolean).join(' · ')}
                      </p>
                    )}
                  </div>
                  {canReview && (
                    <div className="flex shrink-0 gap-1">
                      <ChromeButton
                        size="sm"
                        variant="primary"
                        disabled={review.isPending}
                        onClick={() => review.mutate({ run, approve: true })}
                      >
                        {t('remediation.run.approve')}
                      </ChromeButton>
                      <ChromeButton
                        size="sm"
                        variant="ghost"
                        disabled={review.isPending}
                        onClick={() => review.mutate({ run, approve: false })}
                      >
                        {t('remediation.run.reject')}
                      </ChromeButton>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
      {starting && <StartDialog incident={incident} action={starting} onClose={() => setStarting(null)} />}
    </IncidentSection>
  );
}

/** Filling in an action's parameters and why: it goes for approval. */
function StartDialog({ incident, action, onClose }: { incident: Incident; action: RemediationAction; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [inputs, setInputs] = React.useState<Record<string, string>>(() =>
    Object.fromEntries(action.parameters.map((parameter) => [parameter.key, parameter.default ?? ''])),
  );
  const [reason, setReason] = React.useState('');
  const start = useMutation({
    mutationFn: () =>
      remediationApi.requestRemediation(incident.id, {
        actionId: action.id,
        inputs: Object.fromEntries(Object.entries(inputs).filter(([, value]) => value.trim() !== '')),
        reason: reason.trim(),
      }),
    onSuccess: (result) => {
      toast.success(t('remediation.run.requested', { count: result.approval.requiredApprovals }));
      void queryClient.invalidateQueries({ queryKey: ['incident-remediations', incident.id] });
      void queryClient.invalidateQueries({ queryKey: ['incident-timeline', incident.id] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('remediation.run.title', { name: action.name })}</DialogTitle>
          <DialogDescription>
            {t(action.risk === 'l3' ? 'remediation.run.needs_two' : 'remediation.run.needs_one')}
          </DialogDescription>
        </DialogHeader>
        <form
          id="remediation-start-form"
          data-remediation-start
          className="flex min-w-0 flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            start.mutate();
          }}
        >
          {action.parameters.map((parameter) => (
            <FormField key={parameter.key} label={parameter.label} required={parameter.required}>
              {parameter.kind === 'select' ? (
                <FormSelect
                  value={inputs[parameter.key] ?? ''}
                  onChange={(value) => setInputs((current) => ({ ...current, [parameter.key]: value }))}
                  options={[
                    ...(parameter.required ? [] : [{ value: '', label: '—' }]),
                    ...parameter.options.map((option) => ({ value: option, label: option })),
                  ]}
                  ariaLabel={parameter.label}
                />
              ) : (
                <FormInput
                  type={parameter.kind === 'number' ? 'number' : 'text'}
                  step="any"
                  value={inputs[parameter.key] ?? ''}
                  onChange={(event) => setInputs((current) => ({ ...current, [parameter.key]: event.target.value }))}
                />
              )}
            </FormField>
          ))}
          <FormField label={t('remediation.run.reason')}>
            <FormTextarea
              value={reason}
              rows={2}
              onChange={(event) => setReason(event.target.value)}
              placeholder={t('remediation.run.reason_placeholder')}
            />
          </FormField>
        </form>
        <DialogFooter>
          <ChromeButton type="button" onClick={onClose}>
            {tc('actions.cancel')}
          </ChromeButton>
          <ChromeButton type="submit" form="remediation-start-form" variant="primary" disabled={start.isPending}>
            {t('remediation.run.submit')}
          </ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

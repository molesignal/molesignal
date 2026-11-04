import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { apmApi, apmQueryKeys, type ErrorIssueStatus, type ErrorSummary } from '@/api/apm';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';

import { useTeams } from '../catalog/people';

const UNASSIGNED = '__unassigned__';
const STATUSES: readonly ErrorIssueStatus[] = ['open', 'investigating', 'resolved', 'ignored'];
const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3';

/**
 * What is being done about an error group, set where it is read: open,
 * under investigation, resolved or ignored, and which team and member of
 * the organization have it. A resolved group seen again reads as a
 * regression until it is set again.
 */
export function IssueControls({ group }: { group: ErrorSummary }) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'apm.errors.manage' });
  const { users } = useUsers();
  const { teams } = useTeams();
  const status = group.issue?.status ?? 'open';
  const assignee = group.issue?.assigneeId ?? '';
  const team = group.issue?.teamId ?? '';
  const current = { status, assigneeId: assignee || null, teamId: team || null };

  const save = useMutation({
    mutationFn: (write: { status: ErrorIssueStatus; assigneeId: string | null; teamId: string | null }) =>
      apmApi.saveIssue(group.error.fingerprint, {
        ...write,
        lastSeenAt: group.lastSeenAt,
      }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: apmQueryKeys.all });
      toast.success(t('errors.saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const disabled = access.disabled || save.isPending;

  return (
    <div className="flex items-center gap-2" data-apm-issue-controls="" title={access.reason}>
      <FittedSelect
        aria-label={t('errors.status')}
        data-apm-issue-status=""
        value={status}
        disabled={disabled}
        onValueChange={(next) => save.mutate({ ...current, status: next as ErrorIssueStatus })}
        options={STATUSES.map((value) => ({
          value,
          label: t(`errors.set.${value}`),
        }))}
        maxWidth={160}
        className={CONTROL}
      />
      <FittedSelect
        aria-label={t('errors.team')}
        data-apm-issue-team=""
        value={team || UNASSIGNED}
        disabled={disabled}
        onValueChange={(next) => save.mutate({ ...current, teamId: next === UNASSIGNED ? null : next })}
        options={[{ value: UNASSIGNED, label: t('errors.no_team') }, ...teams.map((item) => ({ value: item.id, label: item.name }))]}
        maxWidth={200}
        className={CONTROL}
      />
      <FittedSelect
        aria-label={t('errors.assignee')}
        data-apm-issue-assignee=""
        value={assignee || UNASSIGNED}
        disabled={disabled}
        onValueChange={(next) => save.mutate({ ...current, assigneeId: next === UNASSIGNED ? null : next })}
        options={[{ value: UNASSIGNED, label: t('errors.unassigned') }, ...users.map((user) => ({ value: user.id, label: user.name }))]}
        maxWidth={200}
        className={CONTROL}
      />
    </div>
  );
}

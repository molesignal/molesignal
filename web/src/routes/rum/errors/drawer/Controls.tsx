import { useTranslation } from 'react-i18next';

import { type RumIssue, type RumIssueChange, type RumIssueStatus, SETTABLE_STATUSES } from '@/api/rum';
import { useActionAccess } from '@/product/actionAccess';
import { useTeams } from '@/routes/apm/catalog/people';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { useUsers } from '@/shell/useUsers';

import { STATE_INK } from '../model';
import { useChangeIssue } from '../useErrors';

const NOBODY = '__nobody__';

const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/**
 * What is being done about an issue, set where it is read: its state, the team that has it and the
 * person. Changing it needs `rum.issues.manage`; without it the controls are only a reading. A state the
 * rules set (regressed) is shown while the issue is in it and cannot be chosen.
 */
export function IssueControls({ issue }: { issue: RumIssue }) {
  const { t } = useTranslation('rum');
  const access = useActionAccess({ permission: 'rum.issues.manage' });
  const { users } = useUsers();
  const { teams } = useTeams();
  const change = useChangeIssue();

  const disabled = access.disabled || change.isPending;

  const apply = (changed: RumIssueChange) => change.mutate({ id: issue.id, change: changed });

  const statuses: RumIssueStatus[] = issue.status === 'regressed' ? ['regressed', ...SETTABLE_STATUSES] : [...SETTABLE_STATUSES];

  return (
    <div data-rum-issue-controls="" title={access.reason} className="flex flex-wrap items-center gap-2">
      <FittedSelect
        aria-label={t('errors.controls.status')}
        data-rum-control="status"
        value={issue.status}
        disabled={disabled}
        onValueChange={(status) => status !== issue.status && apply({ status: status as Exclude<RumIssueStatus, 'regressed'> })}
        options={statuses.map((status) => ({
          value: status,
          label: t(`errors.state.${status}`),
          disabled: status === 'regressed',
        }))}
        maxWidth={160}
        className={cn(CONTROL, STATE_INK[issue.status])}
      />
      <FittedSelect
        aria-label={t('errors.controls.team')}
        data-rum-control="team"
        value={issue.teamId ?? NOBODY}
        disabled={disabled}
        onValueChange={(team) => apply({ teamId: team === NOBODY ? null : team })}
        options={[{ value: NOBODY, label: t('errors.controls.no_team') }, ...teams.map((team) => ({ value: team.id, label: team.name }))]}
        maxWidth={200}
        className={cn(CONTROL, 'text-tx-1')}
      />
      <FittedSelect
        aria-label={t('errors.controls.holder')}
        data-rum-control="holder"
        value={issue.assigneeId ?? NOBODY}
        disabled={disabled}
        onValueChange={(user) => apply({ assigneeId: user === NOBODY ? null : user })}
        options={[{ value: NOBODY, label: t('errors.holder.nobody') }, ...users.map((user) => ({ value: user.id, label: user.name }))]}
        maxWidth={200}
        className={cn(CONTROL, 'text-tx-1')}
      />
    </div>
  );
}

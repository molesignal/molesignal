import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as routingApi from '@/api/alertRouting';
import type { TeamRouting } from '@/api/alertRouting';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { FormSelect } from '@/shell/FormDrawer';
import { serviceName } from '@/shell/incident/Routing';
import { toast } from '@/shell/ui/sonner';

import { SourceSection } from '../sources/Section';
import { alertFlatTableClassName } from '../Surfaces';
import { ROUTING_KEY } from './model';

/** How many services a row names before it counts the rest. */
const NAMED_SERVICES = 3;

/**
 * Each team and the escalation policy its incidents take, beside the services
 * the APM catalog gives it. A team without one is still told; the default
 * policy pages for it.
 */
export function RoutingTeams({
  teams,
  policies,
}: {
  teams: readonly TeamRouting[];
  policies: ReadonlyArray<{ id: string; name: string }>;
}) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.routing.manage' });

  const save = useMutation({
    mutationFn: ({ teamId, policyId }: { teamId: string; policyId: string | null }) =>
      routingApi.setTeamPolicy(teamId, policyId),
    onSuccess: () => {
      toast.success(t('routing.toast_team_policy'));
      void queryClient.invalidateQueries({ queryKey: ROUTING_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const policyOptions = [
    { value: '', label: t('routing.no_policy') },
    ...policies.map((policy) => ({ value: policy.id, label: policy.name })),
  ];

  return (
    <SourceSection title={t('routing.teams_title')} description={t('routing.teams_description')}>
      {teams.length === 0 ? (
        <p className="m-0 px-[20px] pb-[20px] font-sans text-sm text-tx-2">
          {t('routing.teams_empty')}{' '}
          <Link to="/iam/teams" className="text-tx-0 underline-offset-4 outline-none hover:underline focus-visible:underline">
            {t('routing.teams_create')}
          </Link>
        </p>
      ) : (
        <div data-routing-teams>
          <DataTable
            className={`${alertFlatTableClassName} [&_tr>*:first-child]:pl-[20px] [&_tr>*:last-child]:pr-[20px]`}
            rows={[...teams]}
            rowKey={(team) => team.teamId}
            columns={[
              {
                key: 'team',
                header: t('routing.columns.team'),
                width: 220,
                cell: (team) => (
                  <span className="flex min-w-0 items-baseline gap-2">
                    <span className="truncate font-strong text-tx-0">{team.name}</span>
                    <span className="shrink-0 text-tx-2">{t('routing.members', { count: team.memberCount })}</span>
                  </span>
                ),
              },
              {
                key: 'services',
                header: t('routing.columns.services'),
                cell: (team) =>
                  team.services.length === 0 ? (
                    <span className="text-tx-3">{t('routing.services_none')}</span>
                  ) : (
                    <span className="flex min-w-0 items-baseline gap-2">
                      {/* The catalog lists them under the team; the row names the first few. */}
                      <Link
                        to={`/apm/services?team=${encodeURIComponent(team.teamId)}`}
                        className="shrink-0 tabular-nums text-tx-0 underline-offset-4 outline-none hover:underline focus-visible:underline"
                      >
                        {t('routing.services', { count: team.services.length })}
                      </Link>
                      <span className="truncate text-tx-2" title={team.services.map(serviceName).join('\n')}>
                        {team.services.slice(0, NAMED_SERVICES).map(serviceName).join(t('routing.list_separator'))}
                        {team.services.length > NAMED_SERVICES ? t('routing.services_rest') : ''}
                      </span>
                    </span>
                  ),
              },
              {
                key: 'policy',
                header: t('routing.columns.policy'),
                width: 280,
                className: 'overflow-visible',
                cell: (team) => (
                  <FormSelect
                    ariaLabel={t('routing.columns.policy_of', { team: team.name })}
                    value={team.escalationPolicyId ?? ''}
                    disabled={manageAccess.disabled || save.isPending}
                    disabledReason={manageAccess.reason}
                    onChange={(value) => save.mutate({ teamId: team.teamId, policyId: value || null })}
                    options={policyOptions}
                  />
                ),
              },
            ]}
          />
        </div>
      )}
    </SourceSection>
  );
}
